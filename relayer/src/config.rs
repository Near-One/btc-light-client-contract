use anyhow::{Context, Result};
use btc_types::network::Network;
use config::{Config as ConfigBuilder, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "defaults::max_fork_len")]
    pub max_fork_len: u64,
    #[serde(default = "defaults::sleep_time_on_fail_sec")]
    pub sleep_time_on_fail_sec: u64,
    #[serde(default = "defaults::sleep_time_on_reach_last_block_sec")]
    pub sleep_time_on_reach_last_block_sec: u64,
    #[serde(default = "defaults::sleep_time_after_sync_iteration_sec")]
    pub sleep_time_after_sync_iteration_sec: u64,
    #[serde(default = "defaults::fetch_batch_size")]
    pub fetch_batch_size: u64,
    #[serde(default = "defaults::submit_batch_size")]
    pub submit_batch_size: usize,

    pub bitcoin: BitcoinConfig,
    pub near: NearConfig,
    pub init: Option<InitConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinConfig {
    pub endpoint: String,
    pub node_user: Option<String>,
    pub node_password: Option<String>,
    pub node_headers: Option<Vec<(String, String)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearConfig {
    pub endpoint: String,
    pub btc_light_client_account_id: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub private_key: String,
    pub near_credentials_path: Option<PathBuf>,
    #[serde(default = "defaults::transaction_timeout_sec")]
    pub transaction_timeout_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitConfig {
    pub network: Network,
    pub num_of_blcoks_to_submit: u64,
    pub gc_threshold: u64,
    pub skip_pow_verification: bool,
    pub init_height: u64,
}

mod defaults {
    pub fn max_fork_len() -> u64 {
        500
    }
    pub fn sleep_time_on_fail_sec() -> u64 {
        30
    }
    pub fn sleep_time_on_reach_last_block_sec() -> u64 {
        60
    }
    pub fn sleep_time_after_sync_iteration_sec() -> u64 {
        5
    }
    pub fn fetch_batch_size() -> u64 {
        150
    }
    pub fn submit_batch_size() -> usize {
        15
    }
    pub fn transaction_timeout_sec() -> u64 {
        120
    }
}

impl Config {
    /// Load configuration from multiple sources using config-rs
    ///
    /// Priority (highest to lowest):
    /// 1. Environment variables (RELAYER__*, etc.)
    /// 2. Config file (if provided)
    /// 3. Default values
    ///
    /// # Errors
    /// * Configuration build or deserialization error
    /// * `Config::validate` error
    pub fn load(config_file: Option<PathBuf>) -> Result<Self> {
        let mut builder = ConfigBuilder::builder();

        // Add config file if provided
        if let Some(path) = config_file {
            builder = builder.add_source(File::from(path));
        }

        // Environment variables with structured naming
        builder = builder.add_source(
            Environment::with_prefix("RELAYER")
                .separator("__")
                .try_parsing(true),
        );

        let config: Config = builder.build()
            .context("Failed to build configuration")?
            .try_deserialize()
            .context("Failed to load configuration - check required environment variables or config file")?;

        config.validate()?;
        Ok(config)
    }

    /// Validate that all required configuration is present
    fn validate(&self) -> Result<()> {
        let mut missing = Vec::new();

        // Bitcoin node connection is required
        if self.bitcoin.endpoint.is_empty() {
            missing.push("RELAYER_BITCOIN_ENDPOINT (Bitcoin node RPC endpoint)");
        }

        // NEAR configuration is required
        if self.near.endpoint.is_empty() {
            missing.push("RELAYER_NEAR_ENDPOINT (NEAR RPC endpoint)");
        }
        if self.near.btc_light_client_account_id.is_empty() {
            missing.push(
                "RELAYER_NEAR_BTC_LIGHT_CLIENT_ACCOUNT_ID (NEAR light client contract account)",
            );
        }

        // Either private key or credentials file must be provided
        let has_private_key = !self.near.private_key.is_empty();
        let has_credentials_file = self.near.near_credentials_path.is_some();

        if !has_private_key && !has_credentials_file {
            missing.push("Either RELAYER_NEAR_PRIVATE_KEY or RELAYER_NEAR_NEAR_CREDENTIALS_PATH must be provided");
        }

        // If using private key, account ID is required
        if has_private_key && self.near.account_id.is_empty() {
            missing.push("RELAYER_NEAR_ACCOUNT_ID (required when using RELAYER_NEAR_PRIVATE_KEY)");
        }

        if !missing.is_empty() {
            anyhow::bail!(
                "Missing required configuration:\n  - {}\n\nSee example files in the relayer directory or run with --help for more information.",
                missing.join("\n  - ")
            );
        }

        Ok(())
    }

    /// Print configuration summary (hiding sensitive information)
    pub fn print_summary(&self) {
        log::info!("🎯 Relayer Configuration:");
        log::info!("  Bitcoin endpoint: {}", self.bitcoin.endpoint);
        log::info!("  NEAR endpoint: {}", self.near.endpoint);
        log::info!(
            "  Light client contract: {}",
            self.near.btc_light_client_account_id
        );
        log::info!("  Signer account: {}", self.near.account_id);

        if let Some(ref path) = self.near.near_credentials_path {
            log::info!("  Credentials file: {}", path.display());
        } else {
            log::info!("  Using private key authentication");
        }

        log::info!("  Max fork length: {}", self.max_fork_len);
        log::info!("  Fetch batch size: {}", self.fetch_batch_size);
        log::info!("  Submit batch size: {}", self.submit_batch_size);
        log::info!("  Sync sleep: {}s", self.sleep_time_on_reach_last_block_sec);
    }
}
