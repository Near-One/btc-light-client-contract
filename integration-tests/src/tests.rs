use anyhow::Result;
use std::process::Command;

#[cfg(test)]
mod utils;

#[macro_use]
extern crate lazy_static;

lazy_static! {
    static ref INIT: () = {
        env_logger::init();
    };
}

pub fn setup_logger() {
    lazy_static::initialize(&INIT);
}

pub struct RelayerProcess {
    process: std::process::Child,
}

impl RelayerProcess {
    pub fn new(config_path: &str, features: Option<&str>) -> Result<Self> {
        let mut args = vec![
            "run",
            "--bin",
            "btc-relayer",
            "--manifest-path",
            "../relayer/Cargo.toml",
        ];

        if let Some(features) = features {
            args.push("--features");
            args.push(features);
        }

        args.push("--");
        args.push("--config");
        args.push(config_path);

        let process = Command::new("cargo").args(args).spawn()?;

        Ok(Self { process })
    }
}

impl Drop for RelayerProcess {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::utils::{
        create_credentials_file, deploy_contract, init_contract, read_blocks_from_json,
        read_zcash_blocks_from_json,
    };

    use super::*;
    use btc_relayer_lib::config::Config;
    use serde_json::json;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_bitcoin_contract_relayer_integration() -> Result<()> {
        setup_logger();

        let chain = "bitcoin";
        let config_template_path = format!("config/{chain}-template.toml");
        let data_init_path = format!("data/{chain}-init.json");
        let temp_credentials_path = format!("relayer-credentials-{chain}.json");
        let temp_config_path = format!("relayer-config-{chain}.toml");

        // Deploy contract to localnet
        let (contract, user_account, worker) = deploy_contract(chain.to_string()).await?;
        let initial_blocks = read_blocks_from_json(&data_init_path);
        let initial_block_height = 889_056;

        init_contract(&contract, initial_blocks, initial_block_height).await?;

        // Create relayer credentials
        create_credentials_file(&user_account, &temp_credentials_path)?;

        // Create relayer config
        let mut config = Config::new(config_template_path).unwrap();
        config.near.endpoint = worker.rpc_addr();
        config.near.btc_light_client_account_id = contract.id().to_string();
        config.near.near_credentials_path = Some(temp_credentials_path.clone());

        let config_content = toml::to_string(&config).unwrap();
        std::fs::write(&temp_config_path, config_content)?;

        let relayer = RelayerProcess::new(&temp_config_path, None)?;

        // Wait for some time to let relayer process blocks
        sleep(Duration::from_secs(60)).await;

        // Clean up
        drop(relayer);
        let _ = std::fs::remove_file(&temp_credentials_path);
        let _ = std::fs::remove_file(&temp_config_path);

        // Verify blocks processed
        let outcome = contract
            .view("get_mainchain_size")
            .args_json(json!({}))
            .await?;

        assert!(outcome.json::<u64>().unwrap() > 15);

        Ok(())
    }

    #[tokio::test]
    async fn test_litecoin_contract_relayer_integration() -> Result<()> {
        setup_logger();

        let chain = "litecoin";
        let config_template_path = format!("config/{chain}-template.toml");
        let data_init_path = format!("data/{chain}-init.json");
        let temp_credentials_path = format!("relayer-credentials-{chain}.json");
        let temp_config_path = format!("relayer-config-{chain}.toml");

        // Deploy contract to localnet
        let (contract, user_account, worker) = deploy_contract(chain.to_string()).await?;
        let initial_blocks = read_blocks_from_json(&data_init_path);
        let initial_block_height = 2_913_119;

        init_contract(&contract, initial_blocks, initial_block_height).await?;

        // Create relayer credentials
        create_credentials_file(&user_account, &temp_credentials_path)?;

        // Create relayer config
        let mut config = Config::new(config_template_path).unwrap();
        config.near.endpoint = worker.rpc_addr();
        config.near.btc_light_client_account_id = contract.id().to_string();
        config.near.near_credentials_path = Some(temp_credentials_path.clone());

        let config_content = toml::to_string(&config).unwrap();
        std::fs::write(&temp_config_path, config_content)?;

        let relayer = RelayerProcess::new(&temp_config_path, None)?;

        // Wait for some time to let relayer process blocks
        sleep(Duration::from_secs(45)).await;

        // Clean up
        drop(relayer);
        let _ = std::fs::remove_file(&temp_credentials_path);
        let _ = std::fs::remove_file(&temp_config_path);

        // Verify blocks processed
        let outcome = contract
            .view("get_mainchain_size")
            .args_json(json!({}))
            .await?;

        let res = outcome.json::<u64>().unwrap();
        assert!(res > 15);

        Ok(())
    }

    #[tokio::test]
    async fn test_dogecoin_contract_relayer_integration() -> Result<()> {
        setup_logger();

        let chain = "dogecoin";
        let config_template_path = format!("config/{chain}-template.toml");
        let data_init_path = format!("data/{chain}-init.json");
        let temp_credentials_path = format!("relayer-credentials-{chain}.json");
        let temp_config_path = format!("relayer-config-{chain}.toml");

        // Deploy contract to localnet
        let (contract, user_account, worker) = deploy_contract(chain.to_string()).await?;
        let initial_blocks = read_blocks_from_json(&data_init_path);
        let initial_block_height = 5_748_681;

        init_contract(&contract, initial_blocks, initial_block_height).await?;

        // Create relayer credentials
        create_credentials_file(&user_account, &temp_credentials_path)?;

        // Create relayer config
        let mut config = Config::new(config_template_path).unwrap();
        config.near.endpoint = worker.rpc_addr();
        config.near.btc_light_client_account_id = contract.id().to_string();
        config.near.near_credentials_path = Some(temp_credentials_path.clone());

        let config_content = toml::to_string(&config).unwrap();
        std::fs::write(&temp_config_path, config_content)?;

        let relayer = RelayerProcess::new(&temp_config_path, Some(chain))?;

        // Wait for some time to let relayer process blocks
        sleep(Duration::from_secs(45)).await;

        // Clean up
        drop(relayer);
        let _ = std::fs::remove_file(&temp_credentials_path);
        let _ = std::fs::remove_file(&temp_config_path);

        // Verify blocks processed
        let outcome = contract
            .view("get_mainchain_size")
            .args_json(json!({}))
            .await?;

        let res = outcome.json::<u64>().unwrap();
        assert!(res > 5);

        Ok(())
    }

    #[tokio::test]
    async fn test_zcash_contract_relayer_integration() -> Result<()> {
        setup_logger();

        let chain = "zcash";
        let config_template_path = format!("config/{chain}-template.toml");
        let data_init_path = format!("data/{chain}-init.json");
        let temp_credentials_path = format!("relayer-credentials-{chain}.json");
        let temp_config_path = format!("relayer-config-{chain}.toml");

        // Deploy contract to localnet
        let (contract, user_account, worker) = deploy_contract(chain.to_string()).await?;
        let initial_blocks = read_zcash_blocks_from_json(&data_init_path);
        let initial_block_height = 2_940_821;

        init_contract(&contract, initial_blocks, initial_block_height).await?;

        // Create relayer credentials
        create_credentials_file(&user_account, &temp_credentials_path)?;

        // Create relayer config
        let mut config = Config::new(config_template_path).unwrap();
        config.near.endpoint = worker.rpc_addr();
        config.near.btc_light_client_account_id = contract.id().to_string();
        config.near.near_credentials_path = Some(temp_credentials_path.clone());

        let config_content = toml::to_string(&config).unwrap();
        std::fs::write(&temp_config_path, config_content)?;

        let relayer = RelayerProcess::new(&temp_config_path, Some(chain))?;

        // Wait for some time to let relayer process blocks
        sleep(Duration::from_secs(60)).await;

        // Clean up
        drop(relayer);
        let _ = std::fs::remove_file(&temp_credentials_path);
        let _ = std::fs::remove_file(&temp_config_path);

        // Verify blocks processed
        let outcome = contract
            .view("get_mainchain_size")
            .args_json(json!({}))
            .await?;

        let res = outcome.json::<u64>().unwrap();
        assert!(res > 15);

        Ok(())
    }
}
