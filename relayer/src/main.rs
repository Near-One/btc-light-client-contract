use bitcoincore_rpc::bitcoin::hashes::Hash;
use log::{debug, error, info};
use merkle_tools::H256;
use std::env;

use crate::bitcoin_client::Client as BitcoinClient;
use crate::config::Config;
use crate::near_client::{CustomError, NearClient};

#[allow(clippy::single_component_path_imports)]
use merkle_tools;

mod bitcoin_client;
mod config;
mod near_client;

struct Synchronizer {
    bitcoin_client: BitcoinClient,
    near_client: NearClient,
    config: Config,
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
        let mut current_height = self.get_last_correct_block_height().await.unwrap() + 1;

        loop {
            // Get the latest block height from the Bitcoin client
            let latest_height = self.bitcoin_client.get_block_count();

            // Check if we have reached the latest block height
            if current_height >= latest_height {
                // Wait for a certain duration before checking for a new block
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }

            let mut blocks_to_submit = vec![];
            let batch_size = 15;
            for (i, current_height) in (current_height..latest_height).enumerate() {
                if i > batch_size {
                    break;
                }

                let block_hash = self.bitcoin_client.get_block_hash(current_height);
                let block_header = self.bitcoin_client.get_block_header(&block_hash);
                blocks_to_submit.push(block_header);
            }

            let block_to_submit_len: u64 = blocks_to_submit.len().try_into().unwrap();

            info!(
                "Submit blocks with height: [{} - {}]",
                current_height,
                current_height + block_to_submit_len - 1
            );

            match self.near_client.submit_blocks(blocks_to_submit).await {
                Ok(Err(CustomError::PrevBlockNotFound)) => {
                    // Contract cannot save block, because no previous block found, we are in fork
                    current_height = self.get_last_correct_block_height().await.unwrap() + 1;
                }
                Ok(_) => {
                    current_height += block_to_submit_len;
                }
                _ => {
                    // network error after retries
                    panic!("Off-chain relay panics after multiple attempts to save block");
                }
            }
        }
    }

    async fn get_last_correct_block_height(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let last_block_header = self.near_client
            .get_last_block_header()
            .await?;
        let last_block_height = last_block_header.block_height;

        if self.get_bitcoin_block_hash_by_height(last_block_height)
            == last_block_header.current_block_hash.to_string() {
            return Ok(last_block_height);
        } else {
            let amount_of_blocks_to_request = 500_u64;

            let last_block_hashes_in_relay_contract = self
                .near_client
                .get_last_n_blocks_hashes(amount_of_blocks_to_request, 0)
                .await
                .expect("read block header successfully");

            let last_block_hashes_count = last_block_hashes_in_relay_contract.len();

            let mut height: u64 = last_block_height - 1;

            for i in 1..last_block_hashes_count {
                if last_block_hashes_in_relay_contract[last_block_hashes_count - i - 1] ==
                    self.get_bitcoin_block_hash_by_height(height) {
                    return Ok(height);
                }

                height -= 1;
            }
        }

        return Err("The block Height not found".into());
    }

    fn get_bitcoin_block_hash_by_height(&self, height: u64) -> String {
        let block_from_bitcoin_node =
            self.bitcoin_client.get_block_header_by_height(height);

        block_from_bitcoin_node.block_hash().to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = Config::new().expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(&config);
    let near_client = NearClient::new(&config.near);

    // RUNNING IN VERIFICATION MODE
    let verify_mode = env::var("VERIFY_MODE").unwrap_or_default();
    if verify_mode == "true" {
        info!("running transaction verification");
        verify_transaction_flow(bitcoin_client, near_client).await;
        return Ok(());
    }

    // RUNNING IN BLOCK RELAY MODE
    info!("run block header sync");
    let mut synchronizer = Synchronizer::new(bitcoin_client, near_client.clone(), config);
    synchronizer.sync().await;
    info!("end block header sync");

    //near_client.read_last_block_header().await.expect("read block header successfully");

    Ok(())
}

async fn verify_transaction_flow(bitcoin_client: BitcoinClient, near_client: NearClient) {
    // Read the transaction_position from the environment variable
    let transaction_position = env::var("TRANSACTION_POSITION")
        .map(|pos| pos.parse::<usize>().unwrap_or_default())
        .unwrap_or_default();

    // Read the transaction_block_height from the environment variable
    let transaction_block_height = env::var("TRANSACTION_BLOCK_HEIGHT")
        .map(|height| height.parse::<usize>().unwrap_or_default())
        .unwrap_or_default();

    // Read the transaction_block_height from the environment variable
    let force_transaction_hash = env::var("FORCE_TRANSACTION_HASH")
        .map(|hash| hash.parse::<String>().unwrap_or_default())
        .unwrap_or_default();

    let block = bitcoin_client.get_block_by_height(
        u64::try_from(transaction_block_height).expect("correct transaction height"),
    );
    let transaction_block_blockhash = block.header.block_hash();

    let transactions = block
        .txdata
        .iter()
        .map(|tx| H256(tx.compute_txid().to_byte_array()))
        .collect::<Vec<_>>();

    // Provide the transaction hash and merkle proof
    let transaction_hash = transactions[transaction_position].clone(); // Provide the transaction hash
    let merkle_proof = bitcoin_client::Client::compute_merkle_proof(&block, transaction_position); // Provide the merkle proof

    // If we need to force some specific transaction hash
    let transaction_hash = if force_transaction_hash.is_empty() {
        transaction_hash
    } else {
        H256(
            hex::decode(force_transaction_hash)
                .unwrap()
                .try_into()
                .unwrap(),
        )
    };

    let result = near_client
        .verify_transaction_inclusion(
            transaction_hash,
            transaction_position,
            transaction_block_blockhash.to_byte_array().into(),
            merkle_proof,
        )
        .await;

    match result {
        Ok(true) => info!("Transaction is found in the provided block"),
        Ok(false) => info!("Transaction is NOT found in the provided block"),
        Err(e) => error!("Error: {:?}", e),
    }
}
