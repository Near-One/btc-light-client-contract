use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub bitcoin: BitcoinConfig,
    pub near: NearConfig,
}

#[allow(dead_code)]
#[derive(Deserialize, Clone, Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct BitcoinConfig {
    pub endpoint: String,
    pub node_user: String,
    pub node_password: String
}

#[derive(Deserialize, Clone, Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct NearConfig {
    pub endpoint: String,
    pub btc_light_client_account_id: String,
    pub account_name: String,
    pub secret_key: String,
}

/// Launching configuration file from a ./config.toml
/// Expects configuration to be in the same directory as an executable file
impl Config {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config_toml = fs::read_to_string("./config.toml")?;
        let config: Config = toml::from_str(&config_toml)?;
        Ok(config)
    }
}
