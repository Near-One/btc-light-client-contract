#[cfg(feature = "dogecoin")]
mod test_dogecoin {
    use btc_types::aux::AuxData;
    use btc_types::contract_args::InitArgs;
    use btc_types::hash::H256;
    use btc_types::header::Header;
    use btc_types::network::Network;
    use near_sdk::NearToken;
    use near_workspaces::{Account, Contract};
    use serde_json::json;

    const STORAGE_DEPOSIT_PER_BLOCK: NearToken = NearToken::from_millinear(500);
    const DOGE_BITS: u32 = 0x1e0fffff;

    fn doge_genesis() -> Header {
        Header {
            version: 1,
            prev_block_hash: H256::default(),
            merkle_root: H256::default(),
            time: 1_500_000_000,
            bits: DOGE_BITS,
            nonce: 0,
        }
    }

    fn doge_block1() -> Header {
        Header {
            version: 1,
            prev_block_hash: doge_genesis().block_hash(),
            merkle_root: H256::default(),
            time: 1_500_000_060,
            bits: DOGE_BITS,
            nonce: 0,
        }
    }

    fn doge_block_wrong_bits() -> Header {
        Header {
            version: 1,
            prev_block_hash: doge_block1().block_hash(),
            merkle_root: H256::default(),
            time: 1_500_000_120,
            bits: DOGE_BITS - 1,
            nonce: 0,
        }
    }

    async fn compile_dogecoin_wasm() -> Vec<u8> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let status = tokio::process::Command::new("cargo")
            .args([
                "near",
                "build",
                "non-reproducible-wasm",
                "--no-default-features",
                "--features",
                "dogecoin",
            ])
            .current_dir(manifest_dir)
            .status()
            .await
            .expect("Failed to run cargo near build for dogecoin WASM");
        assert!(status.success(), "Failed to build dogecoin WASM");

        let wasm_path =
            format!("{manifest_dir}/target/near/btc_light_client_contract.wasm");
        tokio::fs::read(&wasm_path)
            .await
            .unwrap_or_else(|e| panic!("Failed to read dogecoin WASM at {wasm_path}: {e}"))
    }

    async fn init_dogecoin_contract() -> Result<(Contract, Account), Box<dyn std::error::Error>> {
        let sandbox = near_workspaces::sandbox().await?;
        let wasm = compile_dogecoin_wasm().await;
        let contract = sandbox.dev_deploy(&wasm).await?;

        let genesis = doge_genesis();
        let args = InitArgs {
            genesis_block_hash: genesis.block_hash(),
            genesis_block_height: 0,
            skip_pow_verification: false,
            gc_threshold: 10,
            network: Network::Mainnet,
            submit_blocks: vec![genesis, doge_block1()],
        };

        let outcome = contract
            .call("init")
            .args_json(json!({ "args": serde_json::to_value(args).unwrap() }))
            .transact()
            .await?;
        assert!(outcome.is_success(), "Init failed: {:?}", outcome.failures());

        let user_account = sandbox.dev_create_account().await?;
        Ok((contract, user_account))
    }

    /// Submit a Dogecoin block with wrong target bits and no AuxPoW data.
    /// Expected: rejected with "Error: Incorrect target." before any AuxPoW checks.
    #[tokio::test]
    async fn test_wrong_target_no_auxpow_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let (contract, user_account) = init_dogecoin_contract().await?;

        let headers: Vec<(Header, Option<AuxData>)> = vec![(doge_block_wrong_bits(), None)];
        let outcome = user_account
            .call(contract.id(), "submit_blocks")
            .args_borsh(headers)
            .deposit(STORAGE_DEPOSIT_PER_BLOCK)
            .max_gas()
            .transact()
            .await?;

        assert!(
            format!("{:?}", outcome.failures()[0].clone().into_result())
                .contains("Error: Incorrect target."),
            "Expected 'Error: Incorrect target.' but got: {:?}",
            outcome.failures()
        );
        Ok(())
    }

    /// Submit a Dogecoin block with wrong target bits AND AuxPoW data attached.
    /// Expected: still rejected with "Error: Incorrect target." because the bits
    /// check in check_pow fires before check_aux is ever reached.
    #[tokio::test]
    async fn test_wrong_target_with_auxpow_rejected_before_auxpow_check(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (contract, user_account) = init_dogecoin_contract().await?;

        let aux_data = AuxData {
            coinbase_tx: vec![],
            merkle_proof: vec![],
            chain_merkle_proof: vec![],
            chain_id: 0,
            parent_block: doge_genesis(),
        };
        let headers: Vec<(Header, Option<AuxData>)> =
            vec![(doge_block_wrong_bits(), Some(aux_data))];
        let outcome = user_account
            .call(contract.id(), "submit_blocks")
            .args_borsh(headers)
            .deposit(STORAGE_DEPOSIT_PER_BLOCK)
            .max_gas()
            .transact()
            .await?;

        assert!(
            format!("{:?}", outcome.failures()[0].clone().into_result())
                .contains("Error: Incorrect target."),
            "Expected 'Error: Incorrect target.' but got: {:?}",
            outcome.failures()
        );
        Ok(())
    }
}
