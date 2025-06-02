use btc_types::network::Network;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub max_fork_len: u64,
    pub sleep_time_on_fail_sec: u64,
    pub sleep_time_on_reach_last_block_sec: u64,
    pub sleep_time_after_sync_iteration_sec: u64,
    pub batch_size: usize,
    pub bitcoin: BitcoinConfig,
    pub near: NearConfig,
    pub init: Option<InitConfig>,
}

#[allow(dead_code)]
#[derive(Deserialize, Clone, Debug)]
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
    pub account_name: Option<String>,
    pub secret_key: Option<String>,
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

/// Launching configuration file from a ./config.toml
/// Expects configuration to be in the same directory as an executable file
impl Config {
    /// Parse config
    ///
    /// # Errors
    /// * config file not exists
    /// * incorrect config
    pub fn new(file: String) -> Result<Self, Box<dyn std::error::Error>> {
        let config_toml = fs::read_to_string(file)?;
        let config: Config = toml::from_str(&config_toml)?;
        Ok(config)
    }
}
