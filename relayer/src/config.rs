use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub bitcoin: BitcoinConfig,
    pub near: NearConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BitcoinConfig {
    pub endpoint: String,
    pub node_user: String,
    pub node_password: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct NearConfig {
    pub endpoint: String,
    pub account_name: String,
    pub secret_key: String,
}

impl Config {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config_toml = fs::read_to_string("./config.toml")?;
        let config: Config = toml::from_str(&config_toml)?;
        Ok(config)
    }
}
