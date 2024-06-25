use log::{debug, error, info};
use std::env;

use crate::bitcoin_client::Client as BitcoinClient;
use crate::config::Config;
use crate::near_client::Client as NearClient;

#[allow(clippy::single_component_path_imports)]
use merkle_tools;


mod bitcoin_client;
mod config;
mod near_client;

struct Synchronizer {
    bitcoin_client: BitcoinClient,
    near_client: NearClient,
}

impl Synchronizer {
    pub fn new(bitcoin_client: BitcoinClient, near_client: NearClient) -> Self {
        Self {
            bitcoin_client,
            near_client,
        }
    }
    async fn sync(&mut self) {
        let mut current_height = self.get_block_height();

        loop {
            // Get the latest block height from the Bitcoin client
            let latest_height = self.bitcoin_client.get_block_count();

            // Check if we have reached the latest block height
            if current_height >= latest_height {
                // Wait for a certain duration before checking for a new block
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }

            let block_hash = self.bitcoin_client.get_block_hash(current_height);
            let block_header = self.bitcoin_client.get_block_header(&block_hash);

            match self
                .near_client
                .submit_block_header(block_header)
                .await
            {
                Ok(Err(1)) => {
                    // Contract cannot save block, because no previous block found, we are in fork
                    current_height = self
                        .adjust_height_to_the_fork(current_height)
                        .await;
                }
                Ok(_) => {
                    // Block has been saved
                }
                Err(_) => {
                    // network error after retries
                    panic!("Off-chain relay panics after multiple attempts to save block");
                }
            }

            current_height += 1;
        }
    }

    // Adjust height of the block to start submitting new fork, which might become a new main
    async fn adjust_height_to_the_fork(&self, current_height: u64) -> u64 {
        let mut amount_of_blocks_to_request = 25;

        // If we inspected 10_000 bitcoin blocks and still cannot find
        // the point where fork happened something is very wrong
        // it means it happened 10_000 * 10 minutes = 69 days ago (relayer was down for 69 days?)
        while amount_of_blocks_to_request < 10_000 {
            amount_of_blocks_to_request *= 2;

            let last_block_hashes_in_relay_contract = self
                .near_client
                .receive_last_n_blocks(amount_of_blocks_to_request, 0)
                .await
                .expect("read block header successfully");

            // Starting to look for diverge point from previous block
            let mut height = current_height - 1;

            for _i in 0..amount_of_blocks_to_request {
                let block_from_bitcoin_node =
                    self.bitcoin_client.get_block_header_by_height(height);

                let hash = block_from_bitcoin_node.block_hash().to_string();

                // We found that this is the first block in current bitcoin node state that we also have
                // in our main chain in smart contract state.
                // This is a diverge point. We will start submitting new fork from this point.
                if last_block_hashes_in_relay_contract.contains(&hash) {
                    return height;
                }

                height -= 1;
            }
        }

        0
    }

    fn get_block_height(&self) -> u64 {
        277136
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = Config::new().expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(config.clone());
    let near_client = NearClient::new(config.clone());

    // RUNNING IN VERIFICATION MODE
    let verify_mode = env::var("VERIFY_MODE").unwrap_or_default();
    if verify_mode == "true" {
        info!("running transaction verification");
        verify_transaction_flow(bitcoin_client, near_client).await;
        return Ok(());
    }

    // RUNNING IN BLOCK RELAY MODE
    info!("run block header sync");
    let mut synchronizer = Synchronizer::new(bitcoin_client, near_client.clone());
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

    let block = bitcoin_client.get_block_by_height(transaction_block_height as u64);
    let transactions = block
        .txdata
        .iter()
        .map(|tx| tx.txid().to_string())
        .collect::<Vec<String>>();

    // Provide the transaction hash and merkle proof
    let transaction_hash = transactions[transaction_position].clone(); // Provide the transaction hash
    let merkle_proof = bitcoin_client.compute_merkle_proof(block, transaction_position); // Provide the merkle proof

    // If we need to force some specific transaction hash
    let transaction_hash = if force_transaction_hash.is_empty() {
        transaction_hash
    } else {
        force_transaction_hash
    };
    let result = near_client
        .verify_transaction_inclusion(
            transaction_hash,
            transaction_position,
            transaction_block_height,
            merkle_proof,
        )
        .await;

    match result {
        Ok(true) => info!("Transaction is found in the provided block"),
        Ok(false) => info!("Transaction is NOT found in the provided block"),
        Err(e) => error!("Error: {:?}", e),
    }
}
