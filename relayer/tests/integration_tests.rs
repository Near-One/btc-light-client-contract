use bitcoin::consensus::Encodable;
use bitcoincore_rpc::bitcoin::hashes::Hash;
use log::{debug, error, info};

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
    let config = Config::new().expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(&config);
    let near_client = NearClient::new(&config.near);

    let transaction_position = 0usize;
    let transaction_block_height = 277_136usize;
    let force_transaction: Vec<u8> = vec![];

    // RUNNING IN VERIFICATION MODE
    info!("running transaction verification");
    verify_transaction_flow(
        bitcoin_client,
        near_client,
        transaction_position,
        transaction_block_height,
        force_transaction,
        true,
    )
    .await;
}

#[tokio::test]
#[serial]
async fn verify_incorrect_transaction_test() {
    setup();
    let config = Config::new().expect("we expect config.toml to be next to executable in `./`");

    debug!("Configuration loaded: {:?}", config);

    let bitcoin_client = BitcoinClient::new(&config);
    let near_client = NearClient::new(&config.near);

    let transaction_position = 0usize;
    let transaction_block_height = 277_136usize;
    let force_transaction: Vec<u8> = vec![
        1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 83, 3, 144, 58, 4, 4, 0, 1, 32, 43, 73, 18, 77,
        105, 110, 101, 100, 32, 98, 121, 32, 66, 84, 67, 32, 71, 117, 105, 108, 100, 44, 250, 190,
        109, 109, 55, 2, 141, 84, 72, 228, 111, 116, 208, 231, 59, 139, 194, 117, 97, 237, 245,
        119, 59, 120, 148, 186, 203, 119, 109, 140, 41, 55, 249, 130, 57, 121, 1, 0, 0, 0, 0, 0, 0,
        0, 8, 0, 0, 18, 253, 14, 11, 0, 0, 255, 255, 255, 255, 1, 111, 223, 45, 149, 0, 0, 0, 0,
        25, 118, 169, 20, 39, 161, 241, 39, 113, 222, 92, 195, 183, 57, 65, 102, 75, 37, 55, 193,
        83, 22, 190, 67, 136, 172, 0, 0, 0, 1,
    ];

    // RUNNING IN VERIFICATION MODE
    info!("running transaction verification");
    verify_transaction_flow(
        bitcoin_client,
        near_client,
        transaction_position,
        transaction_block_height,
        force_transaction,
        false,
    )
    .await;
}

async fn verify_transaction_flow(
    bitcoin_client: BitcoinClient,
    near_client: NearClient,
    transaction_position: usize,
    transaction_block_height: usize,
    force_transaction: Vec<u8>,
    expected_value: bool,
) {
    let block = bitcoin_client
        .get_block_by_height(
            u64::try_from(transaction_block_height).expect("correct transaction height"),
        )
        .unwrap();
    let transaction_block_blockhash = block.header.block_hash();

    // Provide the transaction hash and merkle proof
    let mut transaction: Vec<u8> = vec![];
    block.txdata[transaction_position]
        .consensus_encode(&mut transaction)
        .expect("error on tx serialization");
    let merkle_proof = BitcoinClient::compute_merkle_proof(&block, transaction_position); // Provide the merkle proof

    transaction = if force_transaction.len() != 0 {
        force_transaction
    } else {
        transaction
    };

    let result = near_client
        .verify_transaction_inclusion(
            transaction,
            transaction_position,
            transaction_block_blockhash.to_byte_array().into(),
            merkle_proof,
        )
        .await;

    match result {
        Ok(true) => info!("Transaction is found in the provided block"),
        Ok(false) => info!("Transaction is NOT found in the provided block"),
        Err(ref e) => error!("Error: {:?}", e),
    }

    assert_eq!(result.unwrap(), expected_value);
}
