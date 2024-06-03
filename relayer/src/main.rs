use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::Transaction;
use log::{debug, error, info, log_enabled, Level};
use serde_json::{from_slice, json};
use std::env;

use crate::bitcoin_client::Client as BitcoinClient;
use crate::config::Config;
use crate::near_client::Client as NearClient;

use merkle_tools;

mod bitcoin_client;
mod config;
mod near_client;

const GENESIS_BLOCK_HEIGHT: u64 = 0;

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
                // Wait for a certain duration before checking for new blocks
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }

            let block_hash = self.bitcoin_client.get_block_hash(current_height);
            let block_header = self.bitcoin_client.get_block_header(&block_hash);

            // detecting if we might be in fork
            let fork_detected = self.detect_fork(block_hash, block_header, current_height);
            let _ = self.get_first_transaction().await;

            // TODO: It is OK to catch up, but to read everything in this way is not efficient
            // TODO: Add retry logic and more solid error handling
            self.near_client
            .submit_block_header(block_header.clone(), current_height as usize)
            .await
            .expect("failed to submit block header");

            if current_height >= 0 {
                // Only do one iteration for testing purpose
                break;
            }

            current_height += 1;
        }
    }

    // Check if we detected a forking point
    async fn detect_fork(
        &self,
        block_hash: bitcoincore_rpc::bitcoin::BlockHash,
        block_header: Header,
        current_height: u64,
    ) -> bool {
        let near_block_header = self
            .near_client
            .read_last_block_header()
            .await
            .expect("read block header succesfully");

        // TODO: update logic here, check the height of the block instead of the block hash
        if block_header.prev_blockhash != near_block_header.prev_blockhash {
            error!("Fork detected at block height: {}", current_height);
            true
        } else {
            false
        }
    }

    async fn get_first_transaction(&self) -> Transaction {
        let block_hash = self.bitcoin_client.get_block_hash(277136);

        let block = self.bitcoin_client.get_block(&block_hash);
        let root = block.compute_merkle_root().unwrap();
        let transaction = block.txdata[0].clone();

        let txid = transaction.txid();
        let merkle_proof = self.bitcoin_client.compute_merkle_proof(block, 0);

        return transaction;
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

    let verify_mode = env::var("VERIFY_MODE").unwrap_or_default();
    if verify_mode == "true" {
        info!("running transaction verification");
        verify_transaction_flow(bitcoin_client, near_client).await;
        return Ok(());
    }

    info!("run block header sync");
    let mut synchonizer = Synchronizer::new(bitcoin_client, near_client.clone());
    synchonizer.sync().await;
    info!("end block header sync");

    //near_client.read_last_block_header().await.expect("read block header succesfully");

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
