use bitcoin::block::Header;
use serde_json::json;

#[tokio::test]
async fn test_setting_genesis_block() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = near_workspaces::sandbox().await?;
    let contract_wasm = near_workspaces::compile_project("./").await?;

    let contract = sandbox.dev_deploy(&contract_wasm).await?;

    let user_account = sandbox.dev_create_account().await?;

    let block_header = genesis_block_header();

    let outcome = user_account
        .call(contract.id(), "submit_genesis")
        .args_json(json!({
            "block_header": {
                "version": 1,
                "prev_blockhash": "0000000000000000000000000000000000000000000000000000000000000000",
                "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
                "time": 1231006505,
                "bits": 486604799,
                "nonce": 2083236893
            },
            "block_height": 0
        }))
        .transact()
        .await?;
    eprint!("{:?}", outcome);
    assert!(outcome.is_success());

    let user_message_outcome = contract
        .view("get_last_block_header")
        .args_json(json!({}))
        .await?;

    assert_eq!(user_message_outcome.json::<Header>()?, block_header);

    Ok(())
}

#[tokio::test]
async fn test_setting_chain_reorg() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = near_workspaces::sandbox().await?;
    let contract_wasm = near_workspaces::compile_project("./").await?;

    let contract = sandbox.dev_deploy(&contract_wasm).await?;

    let user_account = sandbox.dev_create_account().await?;

    let block_header = genesis_block_header();

    let outcome = user_account
        .call(contract.id(), "submit_genesis")
        .args_json(json!({
            "block_header": {
                "version": 1,
                "prev_blockhash": "0000000000000000000000000000000000000000000000000000000000000000",
                "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
                "time": 1231006505,
                "bits": 486604799,
                "nonce": 2083236893
            },
            "block_height": 0
        }))
        .transact()
        .await?;
    assert!(outcome.is_success());

    let user_message_outcome = contract
        .view("get_last_block_header")
        .args_json(json!({}))
        .await?;

    assert_eq!(user_message_outcome.json::<Header>()?, block_header);

    Ok(())
}

fn genesis_block_header() -> Header {
    let json_value = serde_json::json!({
            "version": 1,
            "prev_blockhash": "0000000000000000000000000000000000000000000000000000000000000000",
            "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
            "time": 1231006505,
            "bits": 486604799,
            "nonce": 2083236893
        });
    let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
    parsed_header
}

// Bitcoin header example
fn block_header_example() -> Header {
    let json_value = serde_json::json!({
            // block_hash: 62703463e75c025987093c6fa96e7261ac982063ea048a0550407ddbbe865345
            "version": 1,
            "prev_blockhash": "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
            "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
            "time": 1231006506,
            "bits": 486604799,
            "nonce": 2083236893
        });
    let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
    parsed_header
}

fn fork_block_header_example() -> Header {
    let json_value = serde_json::json!({
            // "hash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
            //"chainwork": "0000000000000000000000000000000000000000000000000000000200020002",
            "version": 1,
            "merkle_root": "0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098",
            "time": 1231469665,
            "nonce": 2573394689_u32,
            "bits": 486604799,
            "prev_blockhash": "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
        });
    let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
    parsed_header
}

fn fork_block_header_example_2() -> Header {
    let json_value = serde_json::json!({
            // "hash": "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd",
            // "chainwork": "0000000000000000000000000000000000000000000000000000000300030003",
          "version": 1,
          "merkle_root": "9b0fc92260312ce44e74ef369f5c66bbb85848f2eddd5a7a1cde251e54ccfdd5",
          "time": 1231469744,
          "nonce": 1639830024,
          "bits": 486604799,
          "prev_blockhash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
        });
    let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
    parsed_header
}

fn fork_block_header_example_3() -> Header {
    let json_value = serde_json::json!({
            // "hash": "0000000082b5015589a3fdf2d4baff403e6f0be035a5d9742c1cae6295464449",
            // "chainwork": "0000000000000000000000000000000000000000000000000000000400040004",
            "version": 1,
            "merkle_root": "999e1c837c76a1b7fbb7e57baf87b309960f5ffefbf2a9b95dd890602272f644",
            "time": 1231470173,
            "nonce": 1844305925,
            "bits": 486604799,
            "prev_blockhash": "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd",
        });
    let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
    parsed_header
}
