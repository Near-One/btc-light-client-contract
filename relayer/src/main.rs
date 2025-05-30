use std::sync::Arc;

use log::{debug, info, trace, warn};

use crate::bitcoin_client::Client as BitcoinClient;
use crate::config::Config;
use crate::near_client::{CustomError, NearClient};

mod bitcoin_client;
mod config;
mod near_client;

struct Synchronizer {
    bitcoin_client: Arc<BitcoinClient>,
    near_client: NearClient,
    config: Config,
}

macro_rules! continue_on_fail {
    ($res:expr, $msg:expr, $sleep_time:expr, $label:tt) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                warn!(target: "relay", "{}. Error: {}", $msg, e);
                trace!(target: "relay", "Sleep {} secs before next loop", $sleep_time);
                tokio::time::sleep(std::time::Duration::from_secs($sleep_time)).await;
                continue $label;
            }
        }
    };
}

async fn get_block_header(
    bitcoin_client: Arc<BitcoinClient>,
    current_height: u64,
) -> Result<(u64, bitcoin::blockdata::block::Header), u64> {
    let Ok(block_hash) = bitcoin_client.get_block_hash(current_height) else {
        warn!("Failed to get block hash at height {}", current_height);
        return Err(current_height);
    };
    let Ok(block_header) = bitcoin_client.get_block_header(&block_hash) else {
        warn!("Failed to get block header at height {}", current_height);
        return Err(current_height);
    };

    Ok((current_height, block_header))
}

impl Synchronizer {
    pub fn new(
        bitcoin_client: Arc<BitcoinClient>,
        near_client: NearClient,
        config: Config,
    ) -> Self {
        Self {
            bitcoin_client,
            near_client,
            config,
        }
    }

    async fn sync(&mut self) {
        let mut first_block_height_to_submit =
            self.get_last_correct_block_height().await.unwrap() + 1;
        let sleep_time_on_fail_sec = self.config.sleep_time_on_fail_sec;

        'main_loop: loop {
            // Get the latest block height from the Bitcoin client
            let latest_height = continue_on_fail!(self.bitcoin_client.get_block_count(), "Bitcoin Client: Error on get_block_count", sleep_time_on_fail_sec, 'main_loop);

            let mut handlers = Vec::new();
            for current_height in first_block_height_to_submit
                ..=latest_height
                    .min(first_block_height_to_submit.saturating_add(self.config.fetch_batch_size))
            {
                handlers.push(tokio::spawn(get_block_header(
                    self.bitcoin_client.clone(),
                    current_height,
                )));
            }

            let mut blocks_to_submit = Vec::new();
            let mut min_failed_height = None;
            for handler in handlers {
                match handler.await {
                    Ok(Ok((height, block_header))) => {
                        blocks_to_submit.push((height, block_header));
                    }
                    Ok(Err(current_height)) => {
                        warn!("Failed to process block at height {}", current_height);
                        min_failed_height = Some(
                            min_failed_height
                                .map_or(current_height, |min: u64| min.min(current_height)),
                        );
                    }
                    Err(e) => {
                        warn!("Task failed with error: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(sleep_time_on_fail_sec))
                            .await;
                        break 'main_loop;
                    }
                }
            }
            blocks_to_submit.sort_by_key(|(height, _)| *height);

            if let Some(min_failed_height) = min_failed_height {
                blocks_to_submit.retain(|(height, _)| *height < min_failed_height);
            }

            let number_of_blocks_to_submit: u64 = blocks_to_submit.len().try_into().unwrap();

            // Check if we have reached the latest block height
            if number_of_blocks_to_submit == 0 {
                // Wait for a certain duration before checking for a new block
                tokio::time::sleep(std::time::Duration::from_secs(
                    self.config.sleep_time_on_reach_last_block_sec,
                ))
                .await;
                continue;
            }

            let last_block_hash = blocks_to_submit[blocks_to_submit.len() - 1].1.block_hash();

            let block_already_submitted = continue_on_fail!(self.near_client.is_block_hash_exists(last_block_hash).await, "NEAR Client: Error on checking if block already submitted", sleep_time_on_fail_sec, 'main_loop);
            if block_already_submitted {
                info!(target: "relay", "Skip block submission: blocks [{} - {}] already on chain", first_block_height_to_submit, first_block_height_to_submit + number_of_blocks_to_submit - 1);
                first_block_height_to_submit += number_of_blocks_to_submit;
                continue 'main_loop;
            }

            for blocks in blocks_to_submit.chunks(self.config.submit_batch_size) {
                let Some(first_block_height) = blocks.first().map(|(height, _)| *height) else {
                    warn!("No blocks to submit in the current chunk");
                    continue 'main_loop;
                };

                let Some(last_block_height) = blocks.last().map(|(height, _)| *height) else {
                    warn!("No last block height in the current chunk");
                    continue 'main_loop;
                };

                info!(
                    "Submit blocks with height: [{} - {}]",
                    first_block_height, last_block_height,
                );

                match self
                    .near_client
                    .submit_blocks(blocks.iter().map(|(_, header)| *header).collect())
                    .await
                {
                    Ok(Err(CustomError::PrevBlockNotFound)) => {
                        // Contract cannot save block, because no previous block found, we are in fork
                        first_block_height_to_submit = continue_on_fail!(self.get_last_correct_block_height().await, "Error on get_last_correct_block_height", sleep_time_on_fail_sec,  'main_loop)
                            + 1;
                    }
                    Ok(val) => {
                        let _ = continue_on_fail!(val, "Error on block submission.", sleep_time_on_fail_sec,  'main_loop);
                        first_block_height_to_submit = last_block_height + 1;
                    }
                    err => {
                        // network error after retries
                        let _ = continue_on_fail!(err, "Off-chain relay panics after multiple attempts to submit blocks", sleep_time_on_fail_sec,  'main_loop);
                    }
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(
                self.config.sleep_time_after_sync_iteration_sec,
            ))
            .await;
        }
    }

    async fn get_last_correct_block_height(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let last_block_header = self.near_client.get_last_block_header().await?;
        let last_block_height = last_block_header.block_height;
        if self.get_bitcoin_block_hash_by_height(last_block_height)?
            == last_block_header.block_hash.to_string()
        {
            return Ok(last_block_height);
        }
        let last_block_hashes_in_relay_contract = self
            .near_client
            .get_last_n_blocks_hashes(self.config.max_fork_len, 1)
            .await?;

        let last_block_hashes_count = last_block_hashes_in_relay_contract.len();

        let mut height: u64 = last_block_height - 1;

        for i in 0..last_block_hashes_count {
            if last_block_hashes_in_relay_contract[last_block_hashes_count - i - 1]
                == self.get_bitcoin_block_hash_by_height(height)?
            {
                return Ok(height);
            }

            height -= 1;
        }

        Err("The block Height not found".into())
    }

    fn get_bitcoin_block_hash_by_height(
        &self,
        height: u64,
    ) -> Result<String, bitcoincore_rpc::Error> {
        let block_from_bitcoin_node = self.bitcoin_client.get_block_header_by_height(height)?;

        Ok(block_from_bitcoin_node.block_hash().to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = Config::new().expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = Arc::new(BitcoinClient::new(&config));
    let near_client = NearClient::new(&config.near);

    // RUNNING IN BLOCK RELAY MODE
    info!("run block header sync");
    let mut synchronizer = Synchronizer::new(bitcoin_client, near_client.clone(), config);
    synchronizer.sync().await;
    info!("end block header sync");

    //near_client.read_last_block_header().await.expect("read block header successfully");

    Ok(())
}
