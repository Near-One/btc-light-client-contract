use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
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
    bitcoin_client: BitcoinClient,
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

impl Synchronizer {
    pub fn new(bitcoin_client: BitcoinClient, near_client: NearClient, config: Config) -> Self {
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

            let mut blocks_to_submit = vec![];
            for current_height in first_block_height_to_submit..=latest_height {
                if blocks_to_submit.len() >= self.config.batch_size {
                    break;
                }

                let block_hash = continue_on_fail!(self.bitcoin_client.get_block_hash(current_height), "Bitcoin Client: Error on get_block_hash", sleep_time_on_fail_sec,  'main_loop);
                let block_header = continue_on_fail!(self.bitcoin_client.get_block_header(&block_hash), "Bitcoin Client: Error on get_block_header", sleep_time_on_fail_sec,  'main_loop);
                blocks_to_submit.push(block_header);
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

            let last_block_hash = blocks_to_submit[blocks_to_submit.len() - 1].block_hash();

            let block_already_submitted = continue_on_fail!(self.near_client.is_block_hash_exists(last_block_hash.to_string()).await, "NEAR Client: Error on checking if block already submitted", sleep_time_on_fail_sec, 'main_loop);
            if block_already_submitted {
                info!(target: "relay", "Skip block submission: blocks [{} - {}] already on chain", first_block_height_to_submit, first_block_height_to_submit + number_of_blocks_to_submit - 1);
                first_block_height_to_submit += number_of_blocks_to_submit;
                continue 'main_loop;
            }

            info!(
                "Submit blocks with height: [{} - {}]",
                first_block_height_to_submit,
                first_block_height_to_submit + number_of_blocks_to_submit - 1
            );

            match self.near_client.submit_blocks(blocks_to_submit).await {
                Ok(Err(CustomError::PrevBlockNotFound)) => {
                    // Contract cannot save block, because no previous block found, we are in fork
                    first_block_height_to_submit = continue_on_fail!(self.get_last_correct_block_height().await, "Error on get_last_correct_block_height", sleep_time_on_fail_sec,  'main_loop)
                        + 1;
                }
                Ok(val) => {
                    let _ = continue_on_fail!(val, "Error on block submission.", sleep_time_on_fail_sec,  'main_loop);
                    first_block_height_to_submit += number_of_blocks_to_submit;
                }
                err => {
                    // network error after retries
                    let _ = continue_on_fail!(err, "Off-chain relay panics after multiple attempts to submit blocks", sleep_time_on_fail_sec,  'main_loop);
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
    ) -> Result<String, Box<dyn std::error::Error>> {
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
    let header_hash = bitcoin_client.get_block_hash(init_config.init_height).unwrap();
    let mut header = bitcoin_client.get_block_header(&header_hash).unwrap();

    let mut headers = vec![header.clone()];
    for _ in 0..init_config.num_of_blcoks_to_submit {
        header = bitcoin_client
            .get_block_header(&BlockHash::from_byte_array(header.prev_block_hash.0))
            .unwrap();

        headers.push(header.clone());
    }

    headers.reverse();

    let genesis_block_height = init_config.init_height - init_config.num_of_blcoks_to_submit;

    let args: InitArgs = InitArgs {
        genesis_block: headers[0].clone(),
        genesis_block_hash: headers[0].block_hash(),
        genesis_block_height,
        skip_pow_verification: init_config.skip_pow_verification,
        gc_threshold: init_config.gc_threshold,
        network: init_config.network,
        submit_blocks: (headers.len() > 1).then(|| headers[1..].to_vec()),
    };

    info!("Init args: {}", serde_json::to_string(&args).unwrap());
    near_client.init_contract(&args).await.unwrap();
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

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(&config);
    let near_client = NearClient::new(&config.near);

    if args.init_contract {
        let init_config = config.init.clone().expect("Init Config not found");
        init_contract(&bitcoin_client, &near_client, init_config).await;
    }
    // RUNNING IN BLOCK RELAY MODE
    info!("run block header sync");
    let mut synchronizer = Synchronizer::new(bitcoin_client, near_client.clone(), config);
    synchronizer.sync().await;
    info!("end block header sync");

    //near_client.read_last_block_header().await.expect("read block header successfully");

    Ok(())
}
