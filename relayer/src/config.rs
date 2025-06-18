use anyhow::{Context as _, Result};
use btc_types::network::Network;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub max_fork_len: u64,
    pub sleep_time_on_fail_sec: u64,
    pub sleep_time_on_reach_last_block_sec: u64,
    pub sleep_time_after_sync_iteration_sec: u64,
    pub fetch_batch_size: u64,
    pub submit_batch_size: usize,
    pub bitcoin: Option<BitcoinConfig>,
    pub near: NearConfig,
    pub init: Option<InitConfig>,
}

#[allow(dead_code)]
#[derive(Deserialize, Clone, Debug, Default)]
#[allow(clippy::module_name_repetitions)]
pub struct BitcoinConfig {
    pub endpoint: String,
    pub node_user: String,
    pub node_password: String,
    pub node_headers: Option<Vec<(String, String)>>,
}

#[derive(Deserialize, Clone, Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct NearConfig {
    pub endpoint: String,
    pub btc_light_client_account_id: String,
    pub account_id: Option<String>,
    pub private_key: Option<String>,
    pub near_credentials_path: Option<String>,
    pub transaction_timeout_sec: u64,
}

#[derive(Deserialize, Clone, Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct InitConfig {
    pub network: Network,
    pub num_of_blcoks_to_submit: u64,
    pub gc_threshold: u64,
    pub skip_pow_verification: bool,
    pub init_height: u64,
}

fn get_env_var(var: &str) -> Option<String> {
    std::env::var(var).ok()
}

/// Launching configuration file from a ./config.toml
/// Expects configuration to be in the same directory as an executable file
impl Config {
    /// Parse config
    ///
    /// # Errors
    /// * config file not exists
    /// * incorrect config
    pub fn new(file: String) -> Result<Self, Box<dyn std::error::Error>> {
        let config_toml = fs::read_to_string(file).context("Failed to read config file")?;
        let mut config: Config =
            toml::from_str(&config_toml).context("Failed to parse config file")?;

        let bitcoin_config = config.bitcoin.clone().unwrap_or_default();

        config.bitcoin = Some(BitcoinConfig {
            endpoint: get_env_var("ENDPOINT").unwrap_or(bitcoin_config.endpoint),
            node_user: get_env_var("NODE_USER").unwrap_or(bitcoin_config.node_user),
            node_password: get_env_var("NODE_PASSWORD").unwrap_or(bitcoin_config.node_password),
            node_headers: match get_env_var("NODE_HEADERS") {
                Some(s) => Some(serde_json::from_str(&s).context("Failed to parse NODE_HEADERS")?),
                None => bitcoin_config.node_headers,
            },
        });

        config.near.endpoint = get_env_var("NEAR_RPC_HTTP_URL").unwrap_or(config.near.endpoint);
        config.near.account_id = get_env_var("NEAR_ACCOUNT_ID").or(config.near.account_id);
        config.near.private_key = get_env_var("NEAR_PRIVATE_KEY").or(config.near.private_key);

        Ok(config)
    }
}
