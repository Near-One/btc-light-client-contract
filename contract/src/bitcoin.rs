use crate::utils::BlocksGetter;
use crate::{BtcLightClient, BtcLightClientExt, Header, U256};
use btc_types::header::ExtendedHeader;
use btc_types::utils::target_from_bits;
use near_sdk::{near, require};

#[near]
impl BtcLightClient {
    pub(crate) fn check_pow(&self, block_header: &Header, prev_block_header: &ExtendedHeader) {
        let expected_bits = self.get_next_work_required(block_header, prev_block_header);

        require!(
            expected_bits == block_header.bits,
            format!(
                "Error: Incorrect target. Expected bits: {:?}, Actual bits: {:?}",
                expected_bits, block_header.bits
            )
        );
    }

    //https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/pow.cpp#L14
    fn get_next_work_required(
        &self,
        block_header: &Header,
        prev_block_header: &ExtendedHeader,
    ) -> u32 {
        let config = self.get_config();

        if (prev_block_header.block_height + 1) % config.blocks_per_adjustment != 0 {
            if config.pow_allow_min_difficulty_blocks {
                if block_header.time
                    > prev_block_header.block_header.time
                        + 2 * config.pow_target_time_between_blocks_secs
                {
                    return config.proof_of_work_limit_bits;
                } else {
                    let mut current_block_header = prev_block_header.clone();
                    while current_block_header.block_header.bits == config.proof_of_work_limit_bits
                        && current_block_header.block_height % config.blocks_per_adjustment != 0
                    {
                        current_block_header =
                            self.get_prev_header(&current_block_header.block_header);
                    }

                    let last_bits = current_block_header.block_header.bits;
                    return last_bits;
                }
            }
            return prev_block_header.block_header.bits;
        }

        let first_block_height =
            prev_block_header.block_height - (config.blocks_per_adjustment - 1);

        let interval_tail_extend_header = self.get_header_by_height(first_block_height);
        self.calculate_next_work_required(
            prev_block_header,
            interval_tail_extend_header.block_header.time.into(),
        )
    }

    //https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/pow.cpp#L50
    fn calculate_next_work_required(
        &self,
        prev_block_header: &ExtendedHeader,
        first_block_time: i64,
    ) -> u32 {
        let config = self.get_config();
        let prev_block_time = prev_block_header.block_header.time;

        let mut actual_time_taken =
            u64::from(prev_block_time.saturating_sub(first_block_time.try_into().unwrap()));

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
        return expected_bits;
    }
}
