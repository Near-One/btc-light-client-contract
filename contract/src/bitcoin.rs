use crate::utils::BlocksGetter;
use crate::{BtcLightClient, BtcLightClientExt, Header, U256};
use btc_types::header::ExtendedHeader;
use btc_types::utils::target_from_bits;
use near_sdk::{near, require};

#[near]
impl BtcLightClient {
     pub(crate) fn check_target_testnet(
        &self,
        block_header: &Header,
        prev_block_header: &ExtendedHeader,
        config: btc_types::network::NetworkConfig,
    ) {
        let time_diff = block_header
            .time
            .saturating_sub(prev_block_header.block_header.time);
        if time_diff > 2 * config.pow_target_time_between_blocks_secs {
            require!(
                block_header.bits == config.proof_of_work_limit_bits,
                format!(
                    "Error: Incorrect bits. Expected bits: {}; Actual bits: {}",
                    config.proof_of_work_limit_bits, block_header.bits
                )
            );
        } else {
            let mut current_block_header = prev_block_header.clone();
            while current_block_header.block_header.bits == config.proof_of_work_limit_bits
                && current_block_header.block_height % config.blocks_per_adjustment != 0
            {
                current_block_header = self.get_prev_header(&current_block_header.block_header);
            }

            let last_bits = current_block_header.block_header.bits;
            require!(
                last_bits == block_header.bits,
                format!(
                    "Error: Incorrect bits. Expected bits: {}; Actual bits: {}",
                    last_bits, block_header.bits
                )
            );
        }
    }

    pub(crate) fn check_pow(&self, block_header: &Header, prev_block_header: &ExtendedHeader) {
        let config = self.get_config();

        if (prev_block_header.block_height + 1) % config.blocks_per_adjustment != 0 {
            if config.pow_allow_min_difficulty_blocks {
                return self.check_target_testnet(block_header, prev_block_header, config);
            }
            require!(
                block_header.bits == prev_block_header.block_header.bits,
                format!(
                    "Error: Incorrect bits. Expected bits: {}; Actual bits: {}.",
                    prev_block_header.block_header.bits, block_header.bits
                )
            );
            return;
        }

        let first_block_height = prev_block_header.block_height + 1 - config.blocks_per_adjustment;

        let interval_tail_extend_header = self.get_header_by_height(first_block_height);
        let prev_block_time = prev_block_header.block_header.time;

        let mut actual_time_taken = u64::from(
            prev_block_time.saturating_sub(interval_tail_extend_header.block_header.time),
        );

        let max_adjustment_factor: u64 = 4;

        if actual_time_taken < config.expected_time_secs / max_adjustment_factor {
            actual_time_taken = config.expected_time_secs / max_adjustment_factor;
        }
        if actual_time_taken > config.expected_time_secs * max_adjustment_factor {
            actual_time_taken = config.expected_time_secs * max_adjustment_factor;
        }

        let last_target = target_from_bits(prev_block_header.block_header.bits);

        let (mut new_target, new_target_overflow) = last_target.overflowing_mul(actual_time_taken);
        require!(!new_target_overflow, "new target overflow");
        new_target = new_target / U256::from(config.expected_time_secs);

        if new_target > config.pow_limit {
            new_target = config.pow_limit;
        }

        let expected_bits = new_target.target_to_bits();

        require!(
            expected_bits == block_header.bits,
            format!(
                "Error: Incorrect target. Expected bits: {:?}, Actual bits: {:?}",
                expected_bits, block_header.bits
            )
        );
    }
}
