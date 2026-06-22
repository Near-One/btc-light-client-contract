use std::path::PathBuf;

use bitcoincore_rpc::bitcoin::hashes::Hash;
use log::{debug, error, info};
use merkle_tools::H256;

use btc_relayer_lib::bitcoin_client::Client as BitcoinClient;
use btc_relayer_lib::config::Config;
use btc_relayer_lib::near_client::NearClient;
use serial_test::serial;

#[macro_use]
extern crate lazy_static;

lazy_static! {
    static ref INIT: () = {
        env_logger::init();
    };
}

fn setup() {
    lazy_static::initialize(&INIT);
}

#[tokio::test]
#[serial]
async fn verify_correct_transaction_test() {
    setup();
    let config = Config::load(Some(PathBuf::from("config.toml")))
        .expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(&config);
    let near_client = NearClient::new(&config.near);

    let transaction_position = 0usize;
    let transaction_block_height = 277_136usize;
    let force_transaction_hash = String::new();

    // RUNNING IN VERIFICATION MODE
    info!("running transaction verification");
    verify_transaction_flow(
        bitcoin_client,
        near_client,
        transaction_position,
        transaction_block_height,
        force_transaction_hash,
        true,
    )
    .await;
}

#[tokio::test]
#[serial]
async fn verify_incorrect_transaction_test() {
    setup();
    let config = Config::load(Some(PathBuf::from("config.toml")))
        .expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(&config);
    let near_client = NearClient::new(&config.near);

    let transaction_position = 0usize;
    let transaction_block_height = 277_136usize;
    let force_transaction_hash =
        "75a25d63da6063b00cb08f794ad0edb81f2fe7cd1f234b6462ff36d137bfaf19".to_string();

    // RUNNING IN VERIFICATION MODE
    info!("running transaction verification");
    verify_transaction_flow(
        bitcoin_client,
        near_client,
        transaction_position,
        transaction_block_height,
        force_transaction_hash,
        false,
    )
    .await;
}

#[tokio::test]
#[serial]
async fn verify_correct_transaction_v2_test() {
    setup();
    let config = Config::load(Some(PathBuf::from("config.toml")))
        .expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(&config);
    let near_client = NearClient::new(&config.near);

    // Position 1 = first non-coinbase tx, so the coinbase proof is meaningful.
    let transaction_position = 1usize;
    let transaction_block_height = 277_136usize;

    info!("running v2 transaction verification");
    verify_transaction_v2_flow(
        bitcoin_client,
        near_client,
        transaction_position,
        transaction_block_height,
        true,
    )
    .await;
}

async fn verify_transaction_flow(
    bitcoin_client: BitcoinClient,
    near_client: NearClient,
    transaction_position: usize,
    transaction_block_height: usize,
    force_transaction_hash: String,
    expected_value: bool,
) {
    let block = bitcoin_client
        .get_block_by_height(
            u64::try_from(transaction_block_height).expect("correct transaction height"),
        )
        .unwrap();
    let transaction_block_blockhash = block.header.block_hash();

    let transactions = block
        .txdata
        .iter()
        .map(|tx| H256(tx.compute_txid().to_byte_array()))
        .collect::<Vec<_>>();

    // Provide the transaction hash and merkle proof
    let transaction_hash = transactions[transaction_position].clone(); // Provide the transaction hash
    let merkle_proof = BitcoinClient::compute_merkle_proof(&block, transaction_position); // Provide the merkle proof

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
            0,
        )
        .await;

    match result {
        Ok(true) => info!("Transaction is found in the provided block"),
        Ok(false) => info!("Transaction is NOT found in the provided block"),
        Err(ref e) => error!("Error: {:?}", e),
    }

    assert_eq!(result.unwrap(), expected_value);
}

async fn verify_transaction_v2_flow(
    bitcoin_client: BitcoinClient,
    near_client: NearClient,
    transaction_position: usize,
    transaction_block_height: usize,
    expected_value: bool,
) {
    let block = bitcoin_client
        .get_block_by_height(
            u64::try_from(transaction_block_height).expect("correct transaction height"),
        )
        .unwrap();
    let transaction_block_blockhash = block.header.block_hash();

    let transactions: Vec<H256> = block
        .txdata
        .iter()
        .map(|tx| H256(tx.compute_txid().to_byte_array()))
        .collect();

    assert!(
        transaction_position < transactions.len(),
        "transaction_position out of bounds for block at height {transaction_block_height} ({} txs)",
        transactions.len()
    );

    let transaction_hash = transactions[transaction_position].clone();
    let merkle_proof = BitcoinClient::compute_merkle_proof(&block, transaction_position);

    let coinbase_tx_id = transactions[0].clone();
    let coinbase_merkle_proof = BitcoinClient::compute_merkle_proof(&block, 0);

    info!(
        "block at height {} has {} txs; merkle proof depth: tx = {}, coinbase = {}",
        transaction_block_height,
        transactions.len(),
        merkle_proof.len(),
        coinbase_merkle_proof.len(),
    );

    let result = near_client
        .verify_transaction_inclusion_v2(
            transaction_hash,
            transaction_position,
            transaction_block_blockhash.to_byte_array().into(),
            merkle_proof,
            coinbase_tx_id,
            coinbase_merkle_proof,
            0,
        )
        .await;

    match result {
        Ok(true) => info!("Transaction is found in the provided block (v2)"),
        Ok(false) => info!("Transaction is NOT found in the provided block (v2)"),
        Err(ref e) => error!("Error: {:?}", e),
    }

    assert_eq!(result.unwrap(), expected_value);
}
