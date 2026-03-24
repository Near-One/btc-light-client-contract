use log::{info, warn};

use crate::config::Config;

/// Target gas budget per transaction, leaving 50 Tgas headroom from the 300 Tgas max.
const TARGET_GAS_BUDGET: u64 = 250_000_000_000_000;

/// Adaptive batch sizer that proactively adjusts batch size based on observed gas usage.
///
/// - On gas exceeded: halve `batch_size` (multiplicative decrease)
/// - On success: compute optimal `batch_size` from `gas_burnt`, clamp to bounds
///
/// `fetch_size` = `batch_size` * `num_parallel_txs` (capped at `max_fetch_size`)
pub struct AdaptiveBatchSizer {
    max_batch_size: u64,
    max_fetch_size: u64,
    current_batch_size: u64,
    min_batch_size: u64,
    num_parallel_txs: u64,
}

impl AdaptiveBatchSizer {
    /// Create a new `AdaptiveBatchSizer` from config.
    ///
    /// `num_parallel_txs` (N) is computed as `config.fetch_batch_size / config.submit_batch_size`
    /// and stays constant. When `batch_size` changes, `fetch_size` = `batch_size` * N.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        let num_parallel_txs = if config.submit_batch_size > 0 {
            config.fetch_batch_size / config.submit_batch_size
        } else {
            1
        }
        .max(1);

        info!(
            target: "adaptive_batch",
            "Initialized: max_batch_size={}, max_fetch_size={}, num_parallel_txs={}, min_batch_size={}",
            config.submit_batch_size,
            config.fetch_batch_size,
            num_parallel_txs,
            config.min_batch_size,
        );

        Self {
            max_batch_size: config.submit_batch_size,
            max_fetch_size: config.fetch_batch_size,
            current_batch_size: config.submit_batch_size,
            min_batch_size: config.min_batch_size,
            num_parallel_txs,
        }
    }

    #[must_use]
    pub fn current_batch_size(&self) -> u64 {
        self.current_batch_size
    }

    #[must_use]
    pub fn current_fetch_size(&self) -> u64 {
        let computed = self.current_batch_size * self.num_parallel_txs;
        computed.min(self.max_fetch_size)
    }

    /// Called when a transaction fails with "Exceeded the maximum amount of gas".
    /// Halves the batch size (multiplicative decrease).
    pub fn on_gas_exceeded(&mut self) {
        let old = self.current_batch_size;
        self.current_batch_size = (self.current_batch_size / 2).max(self.min_batch_size);

        warn!(
            target: "adaptive_batch",
            "Gas exceeded! Batch size: {} -> {}",
            old,
            self.current_batch_size,
        );
    }

    /// Called on successful transaction with `gas_burnt` and the number of blocks in that tx.
    /// Proactively adjusts `batch_size` based on observed gas cost per block.
    pub fn on_success(&mut self, gas_burnt: u64, num_blocks: u64) {
        if num_blocks == 0 || gas_burnt == 0 {
            return;
        }

        let gas_per_block = gas_burnt / num_blocks;
        if gas_per_block == 0 {
            return;
        }

        let optimal = TARGET_GAS_BUDGET / gas_per_block;
        let optimal = optimal.clamp(self.min_batch_size, self.max_batch_size);

        let old = self.current_batch_size;

        if optimal < self.current_batch_size {
            self.current_batch_size = optimal;
            info!(
                target: "adaptive_batch",
                "Proactive decrease: batch_size {} -> {} (gas_per_block={}, gas_burnt={}, blocks={})",
                old, self.current_batch_size, gas_per_block, gas_burnt, num_blocks,
            );
        } else if optimal > self.current_batch_size {
            self.current_batch_size = optimal;
            info!(
                target: "adaptive_batch",
                "Gas-based increase: batch_size {} -> {} (gas_per_block={}, gas_burnt={}, blocks={})",
                old, self.current_batch_size, gas_per_block, gas_burnt, num_blocks,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(submit_batch: u64, fetch_batch: u64) -> Config {
        Config {
            max_fork_len: 500,
            sleep_time_on_fail_sec: 30,
            sleep_time_on_reach_last_block_sec: 60,
            sleep_time_after_sync_iteration_sec: 5,
            fetch_batch_size: fetch_batch,
            submit_batch_size: submit_batch,
            min_batch_size: 1,
            bitcoin: crate::config::BitcoinConfig {
                endpoint: String::new(),
                node_user: None,
                node_password: None,
                node_headers: None,
            },
            near: crate::config::NearConfig {
                endpoint: String::new(),
                btc_light_client_account_id: String::new(),
                account_id: String::new(),
                private_key: String::new(),
                near_credentials_path: None,
                transaction_timeout_sec: 120,
            },
            init: None,
        }
    }

    #[test]
    fn test_initial_values() {
        let config = test_config(15, 150);
        let sizer = AdaptiveBatchSizer::new(&config);

        assert_eq!(sizer.current_batch_size(), 15);
        assert_eq!(sizer.current_fetch_size(), 150);
    }

    #[test]
    fn test_num_parallel_txs() {
        let config = test_config(10, 100);
        let sizer = AdaptiveBatchSizer::new(&config);

        assert_eq!(sizer.num_parallel_txs, 10);
        assert_eq!(sizer.current_fetch_size(), 100);
    }

    #[test]
    fn test_gas_exceeded_halves() {
        let config = test_config(16, 160);
        let mut sizer = AdaptiveBatchSizer::new(&config);

        sizer.on_gas_exceeded();
        assert_eq!(sizer.current_batch_size(), 8);
        assert_eq!(sizer.current_fetch_size(), 80);

        sizer.on_gas_exceeded();
        assert_eq!(sizer.current_batch_size(), 4);

        sizer.on_gas_exceeded();
        assert_eq!(sizer.current_batch_size(), 2);

        sizer.on_gas_exceeded();
        assert_eq!(sizer.current_batch_size(), 1);

        sizer.on_gas_exceeded();
        assert_eq!(sizer.current_batch_size(), 1);
    }

    #[test]
    fn test_on_success_proactive_decrease() {
        let config = test_config(15, 150);
        let mut sizer = AdaptiveBatchSizer::new(&config);

        sizer.on_success(100_000_000_000_000 * 5, 5);
        assert_eq!(sizer.current_batch_size(), 2);
    }

    #[test]
    fn test_on_success_gas_based_increase() {
        let config = test_config(15, 150);
        let mut sizer = AdaptiveBatchSizer::new(&config);

        sizer.on_gas_exceeded();
        assert_eq!(sizer.current_batch_size(), 7);

        sizer.on_success(5_000_000_000_000 * 3, 3);
        assert_eq!(sizer.current_batch_size(), 15);
    }

    #[test]
    fn test_fetch_size_capped_at_max() {
        let config = test_config(15, 50);
        let sizer = AdaptiveBatchSizer::new(&config);

        assert_eq!(sizer.current_fetch_size(), 45);
    }

    #[test]
    fn test_zero_gas_burnt_ignored() {
        let config = test_config(15, 150);
        let mut sizer = AdaptiveBatchSizer::new(&config);

        sizer.on_success(0, 5);
        assert_eq!(sizer.current_batch_size(), 15);

        sizer.on_success(1000, 0);
        assert_eq!(sizer.current_batch_size(), 15);
    }
}
