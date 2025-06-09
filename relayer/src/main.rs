use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use bitcoin_client::AuxData;
use btc_types::contract_args::InitArgs;
use log::{debug, info, trace, warn};

use crate::bitcoin_client::Client as BitcoinClient;
use crate::config::{Config, InitConfig};
use crate::near_client::{CustomError, NearClient};
use clap::Parser;

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

fn get_block_header(
    bitcoin_client: &Arc<BitcoinClient>,
    current_height: u64,
) -> Result<(u64, btc_types::header::Header, Option<AuxData>), u64> {
    let Ok(block_hash) = bitcoin_client.get_block_hash(current_height) else {
        warn!("Failed to get block hash at height {current_height}");
        return Err(current_height);
    };
    let Ok((block_header, aux_data)) = bitcoin_client.get_aux_block_header(&block_hash) else {
        warn!("Failed to get block header at height {current_height}");
        return Err(current_height);
    };

    Ok((current_height, block_header, aux_data))
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

    async fn fetch_blocks_to_submit(
        &self,
        start_height: u64,
        end_height: u64,
    ) -> Vec<(u64, btc_types::header::Header, Option<AuxData>)> {
        let mut handles = Vec::new();
        for current_height in start_height..=end_height {
            handles.push(tokio::spawn({
                let bitcoin_client = self.bitcoin_client.clone();
                async move { get_block_header(&bitcoin_client, current_height) }
            }));
        }

        let mut blocks = Vec::new();
        let mut min_failed_height = None;

        for handler in handles {
            match handler.await {
                Ok(Ok((height, block_header, aux_data))) => {
                    blocks.push((height, block_header, aux_data));
                }
                Ok(Err(current_height)) => {
                    warn!("Failed to process block at height {current_height}");
                    min_failed_height = Some(
                        min_failed_height
                            .map_or(current_height, |min: u64| min.min(current_height)),
                    );
                }
                Err(e) => {
                    warn!("Task failed with error: {e:?}");
                    tokio::time::sleep(std::time::Duration::from_secs(
                        self.config.sleep_time_on_fail_sec,
                    ))
                    .await;
                    break;
                }
            }
        }

        blocks.sort_by_key(|(height, _, _)| *height);
        if let Some(min_failed_height) = min_failed_height {
            blocks.retain(|(height, _, _)| *height < min_failed_height);
        }

        blocks
    }

    async fn check_submission_skipped(
        &self,
        last_block_hash: &str,
        start_height: u64,
        number_of_blocks: u64,
        first_block_height_to_submit: Arc<AtomicU64>,
    ) -> Result<bool, ()> {
        let block_already_submitted = match self
            .near_client
            .is_block_hash_exists(last_block_hash.to_string())
            .await
        {
            Ok(exists) => exists,
            Err(e) => {
                warn!(target: "relay", "NEAR Client: Error on checking if block already submitted. Error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(
                    self.config.sleep_time_on_fail_sec,
                ))
                .await;
                return Err(());
            }
        };

        if block_already_submitted {
            info!(target: "relay", "Skip block submission: blocks [{} - {}] already on chain", start_height, start_height + number_of_blocks - 1);
            first_block_height_to_submit
                .fetch_add(number_of_blocks, std::sync::atomic::Ordering::SeqCst);
            return Ok(true);
        }

        Ok(false)
    }

    async fn prepare_and_submit_batches(
        self: Arc<Self>,
        blocks_to_submit: Vec<(u64, btc_types::header::Header, Option<AuxData>)>,
        first_block_height_to_submit: Arc<AtomicU64>,
    ) {
        let signed_submit_blocks_txs = match self
            .near_client
            .sign_submit_blocks(blocks_to_submit, self.config.submit_batch_size)
            .await
        {
            Ok(txs) => txs,
            Err(e) => {
                warn!(target: "relay", "NEAR Client: Error on sign submit blocks. Error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(
                    self.config.sleep_time_on_fail_sec,
                ))
                .await;
                return;
            }
        };

        let mut handles = Vec::new();
        for tx in signed_submit_blocks_txs {
            let cloned_self = self.clone();
            let first_block_height_to_submit = first_block_height_to_submit.clone();

            handles.push(tokio::spawn(async move {
            info!(target: "relay", "Submit blocks with height: [{} - {}]", tx.first_block_height, tx.last_block_height);
            match cloned_self.near_client.submit_blocks(tx.signed_tx).await {
                Ok(Err(CustomError::PrevBlockNotFound)) => {
                    let Ok(last_block_height) = cloned_self.get_last_correct_block_height().await else {
                        return Err("Error on get_last_block_height".to_string());
                    };
                    first_block_height_to_submit.store(last_block_height + 1, std::sync::atomic::Ordering::SeqCst);
                }
                Ok(Ok(_)) => {
                    first_block_height_to_submit.store(tx.last_block_height + 1, std::sync::atomic::Ordering::SeqCst);
                }
                Ok(Err(err)) => return Err(format!("Error on block submission (not panic): {err:?}")),
                Err(err) => return Err(format!("Task failed with error: {err:?}")),
            }

            Ok(())
        }));
        }

        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => warn!(target: "relay", "Error on block submission: {e}"),
                Err(e) => warn!(target: "relay", "Task panicked: {e:?}"),
            }
        }
    }

    async fn sync(self: Arc<Self>) {
        let first_block_height_to_submit = Arc::new(AtomicU64::new(
            self.get_last_correct_block_height().await.unwrap() + 1,
        ));

        'main_loop: loop {
            let latest_height = continue_on_fail!(
                self.bitcoin_client.get_block_count(),
                "Bitcoin Client: Error on get_block_count",
                self.config.sleep_time_on_fail_sec,
                'main_loop
            );

            let start_height =
                first_block_height_to_submit.load(std::sync::atomic::Ordering::Relaxed);
            let end_height =
                latest_height.min(start_height.saturating_add(self.config.fetch_batch_size));

            let blocks_to_submit = self.fetch_blocks_to_submit(start_height, end_height).await;

            if blocks_to_submit.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(
                    self.config.sleep_time_on_reach_last_block_sec,
                ))
                .await;
                continue;
            }

            let number_of_blocks = blocks_to_submit.len().try_into().unwrap();
            let last_block_hash = blocks_to_submit.last().unwrap().1.block_hash().to_string();

            if let Ok(true) = self
                .check_submission_skipped(
                    &last_block_hash,
                    start_height,
                    number_of_blocks,
                    first_block_height_to_submit.clone(),
                )
                .await
            {
                continue;
            }

            self.clone()
                .prepare_and_submit_batches(blocks_to_submit, first_block_height_to_submit.clone())
                .await;

            tokio::time::sleep(std::time::Duration::from_secs(
                self.config.sleep_time_after_sync_iteration_sec,
            ))
            .await;
        }
    }

    async fn get_last_correct_block_height(
        &self,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
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
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let block_from_bitcoin_node = self.bitcoin_client.get_block_header_by_height(height)?;

        Ok(block_from_bitcoin_node.block_hash().to_string())
    }
}

async fn init_contract(
    bitcoin_client: &BitcoinClient,
    near_client: &NearClient,
    init_config: InitConfig,
) {
    info!("Init contract");

    let header_hash = bitcoin_client
        .get_block_hash(init_config.init_height)
        .expect("Failed to get block hash");

    let mut headers = Vec::with_capacity(
        usize::try_from(init_config.num_of_blcoks_to_submit)
            .expect("Error on converting num_of_blocks_to_submit to usize"),
    );
    let mut current_header = bitcoin_client
        .get_aux_block_header(&header_hash)
        .expect("Failed to get initial block header")
        .0;

    headers.push(current_header.clone());

    for _ in 1..init_config.num_of_blcoks_to_submit {
        let prev_hash = BlockHash::from_byte_array(current_header.prev_block_hash.0);
        current_header = bitcoin_client
            .get_aux_block_header(&prev_hash)
            .expect("Failed to get previous block header")
            .0;
        headers.push(current_header.clone());
    }

    headers.reverse();

    let genesis_block_height = init_config.init_height - init_config.num_of_blcoks_to_submit + 1;

    let args = InitArgs {
        genesis_block_hash: headers[0].block_hash(),
        genesis_block_height,
        skip_pow_verification: init_config.skip_pow_verification,
        gc_threshold: init_config.gc_threshold,
        network: init_config.network,
        submit_blocks: headers,
    };

    info!(
        "Init args: {}",
        serde_json::to_string(&args).unwrap_or_else(|_| "<failed to serialize args>".into())
    );

    near_client
        .init_contract(&args)
        .await
        .expect("Failed to init contract");
}

#[derive(Parser)]
struct CliArgs {
    /// Path to the configuration file
    #[clap(short, long, default_value = "config.toml")]
    config: String,
    /// Initialize contract
    #[clap(long)]
    init_contract: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = CliArgs::parse();

    let config =
        Config::new(args.config).expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {config:?}");

    let bitcoin_client = Arc::new(BitcoinClient::new(&config));
    let near_client = NearClient::new(&config.near);

    if args.init_contract {
        let init_config = config.init.clone().expect("Init Config not found");
        init_contract(&bitcoin_client, &near_client, init_config).await;
    }
    // RUNNING IN BLOCK RELAY MODE
    info!("run block header sync");
    let synchronizer = Arc::new(Synchronizer::new(
        bitcoin_client,
        near_client.clone(),
        config,
    ));
    synchronizer.sync().await;
    info!("end block header sync");

    //near_client.read_last_block_header().await.expect("read block header successfully");

    Ok(())
}
