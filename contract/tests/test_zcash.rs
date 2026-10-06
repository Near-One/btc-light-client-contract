#[cfg(feature = "zcash")]
mod test_zcash {
    use btc_types::contract_args::InitArgs;
    use btc_types::header::{ExtendedHeader, Header};
    use near_sdk::NearToken;
    use near_workspaces::{cargo_near_build, Account, Contract};
    use serde_json::json;
    use std::fs::File;
    use std::io::BufReader;
    use std::str::FromStr;

    const STORAGE_DEPOSIT_PER_BLOCK: NearToken = NearToken::from_millinear(500);

    async fn build_contract() -> Vec<u8> {
        let artifact = cargo_near_build::build_with_cli(cargo_near_build::BuildOpts {
            manifest_path: Some(
                cargo_near_build::camino::Utf8PathBuf::from_str("./Cargo.toml")
                    .expect("camino PathBuf from str"),
            ),
            no_default_features: true,
            features: Some("zcash".to_string()),
            ..Default::default()
        })
        .unwrap_or_else(|e| panic!("building contract: {:?}", e));

        let file = artifact.canonicalize().unwrap();
        std::fs::read(&file).unwrap()
    }

    /// Grant the `UnrestrictedSubmitBlocks` role to an account so it passes the
    /// `#[trusted_relayer]` guard on `submit_blocks`. The contract itself is the
    /// super admin (set during `init`), so it can grant any role.
    async fn grant_relayer_role(
        contract: &Contract,
        account: &Account,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let outcome = contract
            .call("acl_grant_role")
            .args_json(json!({
                "role": "UnrestrictedSubmitBlocks",
                "account_id": account.id(),
            }))
            .transact()
            .await?;
        assert!(
            outcome.is_success(),
            "Failed to grant role: {:?}",
            outcome.failures()
        );
        Ok(())
    }

    async fn init_zcash_contract() -> Result<(Contract, Account), Box<dyn std::error::Error>> {
        let sandbox = near_workspaces::sandbox().await?;
        let contract_wasm = build_contract().await;

        let contract = sandbox.dev_deploy(&contract_wasm).await?;

        let initial_blocks = read_zcash_blocks();
        let genesis_block = initial_blocks[0].clone();

        let args = InitArgs {
            genesis_block_hash: genesis_block.block_hash(),
            genesis_block_height: 2940821,
            skip_pow_verification: false,
            gc_threshold: 2000,
            network: btc_types::network::Network::Mainnet,
            genesis_block: genesis_block,
        };

        let outcome = contract
            .call("init")
            .args_json(json!({
                "args": serde_json::to_value(args).unwrap(),
            }))
            .max_gas()
            .transact()
            .await?;
        assert!(outcome.is_success(), "{:?}", outcome.failures());

        let user_account = sandbox.dev_create_account().await?;
        grant_relayer_role(&contract, &user_account).await?;

        // 11 blocks for MTP + 17 for the pre-NU7 averaging window
        let outcome = submit(&contract, &user_account, initial_blocks[1..29].to_vec()).await?;
        assert!(outcome.is_success(), "{:?}", outcome.failures());

        Ok((contract, user_account))
    }

    async fn submit(
        contract: &Contract,
        relayer: &Account,
        headers: Vec<Header>,
    ) -> Result<near_workspaces::result::ExecutionFinalResult, Box<dyn std::error::Error>> {
        let num_headers = u128::try_from(headers.len()).unwrap();
        Ok(relayer
            .call(contract.id(), "submit_blocks")
            .args_borsh(headers)
            .deposit(STORAGE_DEPOSIT_PER_BLOCK.saturating_mul(num_headers))
            .max_gas()
            .transact()
            .await?)
    }

    fn read_zcash_blocks() -> Vec<Header> {
        let file =
            File::open("./tests/data/zcash_initial_blocks.json").expect("Unable to open file");
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).expect("Unable to parse JSON")
    }

    /// Fetches the wasm currently deployed on a mainnet account via plain RPC.
    /// (`near_workspaces::mainnet()` is not used because its client fails to
    /// parse responses from current mainnet nodes.)
    async fn fetch_mainnet_wasm(account_id: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        use base64::Engine;

        let response: serde_json::Value = reqwest::Client::new()
            .post("https://rpc.mainnet.near.org")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "dontcare",
                "method": "query",
                "params": {
                    "request_type": "view_code",
                    "finality": "final",
                    "account_id": account_id,
                },
            }))
            .send()
            .await?
            .json()
            .await?;

        let code_base64 = response["result"]["code_base64"]
            .as_str()
            .unwrap_or_else(|| panic!("no code_base64 in response: {response}"));
        Ok(base64::engine::general_purpose::STANDARD.decode(code_base64)?)
    }

    /// Initializes a sandbox contract from the wasm currently deployed on
    /// mainnet (`zcash-client.bridge.near`)
    #[tokio::test]
    async fn test_migration_from_mainnet_wasm() -> Result<(), Box<dyn std::error::Error>> {
        let sandbox = near_workspaces::sandbox().await?;
        let old_wasm = fetch_mainnet_wasm("zcash-client.bridge.near").await?;

        let contract = sandbox.dev_deploy(&old_wasm).await?;

        // The mainnet wasm still takes the pre-bootstrap `InitArgs` layout.
        let initial_blocks = read_zcash_blocks();
        let outcome = contract
            .call("init")
            .args_json(json!({ "args": {
                "genesis_block_hash": initial_blocks[0].block_hash(),
                "genesis_block_height": 2940821,
                "skip_pow_verification": false,
                "gc_threshold": 2000,
                "network": btc_types::network::Network::Mainnet,
                "submit_blocks": initial_blocks[..29],
            }}))
            .max_gas()
            .transact()
            .await?;
        assert!(outcome.is_success(), "{:?}", outcome.failures());

        // Upgrade to the current wasm.
        let new_wasm = build_contract().await;
        contract
            .as_account()
            .deploy(&new_wasm)
            .await?
            .into_result()?;

        let outcome = contract
            .call("migrate")
            .args_json(json!({}))
            .max_gas()
            .transact()
            .await?;
        assert!(outcome.is_success(), "{:?}", outcome.failures());

        // State is intact after the migration and the contract is functional.
        let last_header = contract
            .view("get_last_block_header")
            .args_json(json!({}))
            .await?
            .json::<ExtendedHeader>()?;
        assert_eq!(last_header.block_header, initial_blocks[28].clone().into());

        let user_account = sandbox.dev_create_account().await?;
        grant_relayer_role(&contract, &user_account).await?;
        let outcome = user_account
            .call(contract.id(), "submit_blocks")
            .args_borsh(initial_blocks[29..32].to_vec())
            .max_gas()
            .deposit(STORAGE_DEPOSIT_PER_BLOCK.saturating_mul(3))
            .transact()
            .await?;
        assert!(outcome.is_success(), "{:?}", outcome.failures());

        Ok(())
    }

    #[tokio::test]
    async fn test_init() -> Result<(), Box<dyn std::error::Error>> {
        let (contract, _user_account) = init_zcash_contract().await?;

        let outcome = contract
            .view("get_last_block_header")
            .args_json(json!({}))
            .await?;

        let blocks = read_zcash_blocks();
        assert_eq!(
            outcome.json::<ExtendedHeader>()?.block_header,
            blocks[28].clone().into()
        );

        Ok(())
    }

    /// Bootstraps 11 + 102 blocks right before the testnet NU7 activation in
    /// relayer-sized batches; the next block is fully checked.
    #[tokio::test]
    async fn test_bootstrap_before_nu7_activation() -> Result<(), Box<dyn std::error::Error>> {
        let sandbox = near_workspaces::sandbox().await?;
        let contract = sandbox.dev_deploy(&build_contract().await).await?;

        let config = btc_types::network::get_zcash_config(btc_types::network::Network::Testnet);
        let activation_height = config.nu7_activation_height.unwrap();
        let num_bootstrap_blocks = 1 + 11 + 102;

        let mut headers: Vec<Header> = vec![read_zcash_blocks()[0].clone()];
        for _ in 0..num_bootstrap_blocks {
            let prev = headers.last().unwrap();
            let mut header = prev.clone();
            header.prev_block_hash = prev.block_hash();
            header.time = prev.time + 25;
            headers.push(header);
        }

        let args = InitArgs {
            genesis_block_hash: headers[0].block_hash(),
            genesis_block_height: activation_height - u64::try_from(num_bootstrap_blocks).unwrap(),
            skip_pow_verification: false,
            gc_threshold: 2000,
            network: btc_types::network::Network::Testnet,
            genesis_block: headers[0].clone(),
        };
        let outcome = contract
            .call("init")
            .args_json(json!({ "args": serde_json::to_value(args).unwrap() }))
            .max_gas()
            .transact()
            .await?;
        assert!(outcome.is_success(), "{:?}", outcome.failures());

        let user_account = sandbox.dev_create_account().await?;
        grant_relayer_role(&contract, &user_account).await?;

        let mut max_batch_gas = 0;
        for batch in headers[1..num_bootstrap_blocks].chunks(15) {
            let outcome = submit(&contract, &user_account, batch.to_vec()).await?;
            assert!(outcome.is_success(), "{:?}", outcome.failures());
            max_batch_gas = max_batch_gas.max(outcome.total_gas_burnt.as_tgas());
        }
        println!("max bootstrap batch gas: {max_batch_gas} Tgas");

        let last_header = contract
            .view("get_last_block_header")
            .args_json(json!({}))
            .await?
            .json::<ExtendedHeader>()?;
        assert_eq!(last_header.block_height, activation_height - 1);

        // The synthetic block at the activation height has no valid Equihash solution
        let outcome = submit(
            &contract,
            &user_account,
            vec![headers[num_bootstrap_blocks].clone()],
        )
        .await?;
        assert!(outcome.is_failure());

        Ok(())
    }

    #[tokio::test]
    async fn test_block_submission() -> Result<(), Box<dyn std::error::Error>> {
        let (contract, user_account) = init_zcash_contract().await?;

        let blocks = read_zcash_blocks();

        let outcome = user_account
            .call(contract.id(), "submit_blocks")
            .args_borsh(blocks[29..32].to_vec())
            .max_gas()
            .deposit(STORAGE_DEPOSIT_PER_BLOCK.saturating_mul(3))
            .transact()
            .await?;

        assert!(outcome.is_success());

        let last_header = contract
            .view("get_last_block_header")
            .args_json(json!({}))
            .await?
            .json::<ExtendedHeader>()?;
        assert_eq!(last_header.block_header, blocks[31].clone().into());

        assert_eq!(last_header.block_height, 2940852);

        Ok(())
    }

    #[tokio::test]
    async fn test_block_submission_out_of_order() -> Result<(), Box<dyn std::error::Error>> {
        let (contract, user_account) = init_zcash_contract().await?;

        let blocks = read_zcash_blocks();

        let outcome = user_account
            .call(contract.id(), "submit_blocks")
            .args_borsh([blocks[30].clone()].to_vec())
            .max_gas()
            .deposit(STORAGE_DEPOSIT_PER_BLOCK.saturating_mul(3))
            .transact()
            .await?;

        assert!(outcome.is_failure());

        assert!(format!("{:?}", outcome.failures()[0].clone().into_result())
            .contains("PrevBlockNotFound"));

        Ok(())
    }

    #[tokio::test]
    async fn test_block_submission_invalid_target() -> Result<(), Box<dyn std::error::Error>> {
        let (contract, user_account) = init_zcash_contract().await?;

        let blocks = read_zcash_blocks();
        let mut invalid_block = blocks[29].clone();
        invalid_block.bits += 1;

        let outcome = user_account
            .call(contract.id(), "submit_blocks")
            .args_borsh([invalid_block].to_vec())
            .max_gas()
            .deposit(STORAGE_DEPOSIT_PER_BLOCK.saturating_mul(3))
            .transact()
            .await?;

        assert!(outcome.is_failure());

        assert!(format!("{:?}", outcome.failures()[0].clone().into_result())
            .contains("bad-diffbits: incorrect proof of work"));

        Ok(())
    }
}
