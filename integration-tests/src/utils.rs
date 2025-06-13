use anyhow::Result;
use btc_types::network::Network;
use btc_types::zcash_header::Header as ZcashHeader;
use btc_types::{hash::H256, header::Header};
use near_workspaces::{cargo_near_build, network::Sandbox, Account, Contract, Worker};
use serde::Serialize;
use std::{fs::File, io::BufReader, str::FromStr};

async fn build_contract(chain: String) -> Result<Vec<u8>> {
    let artifact = cargo_near_build::build_with_cli(cargo_near_build::BuildOpts {
        manifest_path: Some(
            cargo_near_build::camino::Utf8PathBuf::from_str("../contract/Cargo.toml")
                .expect("camino PathBuf from str"),
        ),
        no_default_features: true,
        features: Some(chain),
        ..Default::default()
    })
    .unwrap_or_else(|e| panic!("building contract: {:?}", e));

    let file = artifact.canonicalize().unwrap();
    Ok(std::fs::read(&file)?)
}

pub async fn deploy_contract(chain: String) -> Result<(Contract, Account, Worker<Sandbox>)> {
    let sandbox = near_workspaces::sandbox().await?;
    let contract_wasm = build_contract(chain).await?;

    let contract = sandbox.dev_deploy(&contract_wasm).await?;
    let user_account = sandbox.dev_create_account().await?;

    Ok((contract, user_account, sandbox))
}

pub trait InitHeader: Clone + Serialize {
    fn block_hash(&self) -> H256;
}

impl InitHeader for Header {
    fn block_hash(&self) -> H256 {
        self.block_hash()
    }
}

impl InitHeader for ZcashHeader {
    fn block_hash(&self) -> H256 {
        self.block_hash()
    }
}

pub async fn init_contract(
    contract: &Contract,
    initial_blocks: Vec<impl InitHeader>,
    initial_block_height: u64,
) -> Result<()> {
    let outcome = contract
        .call("init")
        .args_json(serde_json::json!({
            "args": serde_json::json!({
                "genesis_block_hash": initial_blocks[0].block_hash(),
                "genesis_block_height": initial_block_height,
                "skip_pow_verification": false,
                "gc_threshold": 2000,
                "network": Network::Mainnet,
                "submit_blocks": initial_blocks.clone(),
            }),
        }))
        .max_gas()
        .transact()
        .await?;

    assert!(outcome.is_success());
    Ok(())
}

pub fn read_blocks_from_json(path: &str) -> Vec<Header> {
    let file = File::open(path).expect("Unable to open file");
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).unwrap()
}

pub fn read_zcash_blocks_from_json(path: &str) -> Vec<ZcashHeader> {
    let file = File::open(path).expect("Unable to open file");
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).unwrap()
}

pub fn create_credentials_file(user_account: &Account, path: &str) -> Result<()> {
    let credentials_content = format!(
        r#"
        {{
            "account_id": "{}",
            "public_key": "{}",
            "private_key": "{}"
        }}
        "#,
        user_account.id(),
        user_account.secret_key().public_key(),
        user_account.secret_key().to_string(),
    );

    std::fs::write(path, credentials_content)?;

    Ok(())
}
