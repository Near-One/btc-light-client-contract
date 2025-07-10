use crate::{utils::BlocksGetter, BtcLightClient, BtcLightClientExt};
use btc_types::{
    header::{ExtendedHeader, Header},
    network::{Network, ZcashConfig, MAX_FUTURE_BLOCK_TIME_LOCAL, MAX_FUTURE_BLOCK_TIME_MTP},
    u256::U256,
    utils::target_from_bits,
};
use near_sdk::{env, near, require};

#[near]
impl BtcLightClient {
    pub fn get_config(&self) -> btc_types::network::ZcashConfig {
        btc_types::network::get_zcash_config(self.network)
    }

    pub fn get_network(&self) -> (String, Network) {
        ("Zcash".to_owned(), self.network)
    }

    // Reference implementation: https://github.com/zcash/zcash/blob/v6.2.0/src/main.cpp#L5019
    pub(crate) fn check_pow(&self, block_header: &Header, prev_block_header: &ExtendedHeader) {
        let next_work_result =
            zcash_get_next_work_required(&self.get_config(), block_header, prev_block_header, self);

        require!(
            next_work_result.expected_bits == block_header.bits,
            format!(
                "Error: Incorrect target. Expected bits: {:?}, Actual bits: {:?}",
                next_work_result.expected_bits, block_header.bits
            )
        );

        // Check timestamp against prev
        require!(
            block_header.time >= next_work_result.prev_block_median_time_past,
            "Block time is before the median time of the previous block"
        );

        // Check future timestamp soft fork rule introduced in v2.1.1-1.
        // This retrospectively activates at block height 2 for mainnet and regtest,
        // and 6 blocks after Blossom activation for testnet.
        //
        // MAX_FUTURE_BLOCK_TIME_MTP is typically 129600 seconds (36 hours) in Zcash
        require!(
            block_header.time
                <= next_work_result.prev_block_median_time_past + MAX_FUTURE_BLOCK_TIME_MTP,
            "Block timestamp is too far ahead of median-time-past"
        );

        // Check timestamp
        let current_timestamp = u32::try_from(env::block_timestamp_ms() / 1000).unwrap(); // Convert to seconds
        require!(
            block_header.time <= current_timestamp + MAX_FUTURE_BLOCK_TIME_LOCAL,
            "Block timestamp is too far ahead of local time"
        );

        require!(
            block_header.version >= 4,
            "Block version must be at least 4"
        );

        // Check Equihash solution
        let n = 200;
        let k = 9;
        let input = block_header.get_block_header_vec_for_equihash();

        equihash::is_valid_solution(n, k, &input, &block_header.nonce.0, &block_header.solution)
            .unwrap_or_else(|e| {
                env::panic_str(&format!("Invalid Equihash solution: {e}"));
            });
    }
}

struct NextWorkResult {
    expected_bits: u32,
    prev_block_median_time_past: u32,
}

// Reference implementation: https://github.com/zcash/zcash/blob/v6.2.0/src/pow.cpp#L20
fn zcash_get_next_work_required(
    config: &ZcashConfig,
    block_header: &Header,
    prev_block_header: &ExtendedHeader,
    prev_block_getter: &impl BlocksGetter,
) -> NextWorkResult {
    use btc_types::network::ZCASH_MEDIAN_TIME_SPAN;

    // Find the first block in the averaging interval
    // and the median time past for the first and last blocks in the interval
    let mut current_header = prev_block_header.clone();
    let mut total_target = U256::ZERO;
    let mut median_time = [0u32; ZCASH_MEDIAN_TIME_SPAN];

    let prev_block_median_time_past = {
        for i in 0..usize::try_from(config.pow_averaging_window).unwrap() {
            if i < ZCASH_MEDIAN_TIME_SPAN {
                median_time[i] = current_header.block_header.time;
            }

            let (sum, overflow) =
                total_target.overflowing_add(target_from_bits(current_header.block_header.bits));
            require!(!overflow, "Addition of U256 values overflowed");
            total_target = sum;

            current_header = prev_block_getter.get_prev_header(&current_header.block_header);
        }

        median_time.sort_unstable();
        median_time[median_time.len() / 2]
    };

    let first_block_in_interval_median_time_past = {
        for i in 0..ZCASH_MEDIAN_TIME_SPAN {
            median_time[i] = current_header.block_header.time;
            current_header = prev_block_getter.get_prev_header(&current_header.block_header);
        }
        median_time.sort_unstable();
        median_time[median_time.len() / 2]
    };

    if let Some(pow_allow_min_difficulty_blocks_after_height) =
        config.pow_allow_min_difficulty_blocks_after_height
    {
        // Comparing with >= because this function returns the work required for the block after prev_block_header
        if prev_block_header.block_height >= pow_allow_min_difficulty_blocks_after_height {
            // Special difficulty rule for testnet:
            // If the new block's timestamp is more than 6 * block interval minutes
            // then allow mining of a min-difficulty block.
            if i64::from(block_header.time)
                > i64::from(prev_block_header.block_header.time) + config.pow_target_spacing() * 6
            {
                return NextWorkResult {
                    expected_bits: config.proof_of_work_limit_bits,
                    prev_block_median_time_past,
                };
            }
        }
    }

    // The protocol specification leaves MeanTarget(height) as a rational, and takes the floor
    // only after dividing by AveragingWindowTimespan in the computation of Threshold(height):
    // <https://zips.z.cash/protocol/protocol.pdf#diffadjustment>
    //
    // Here we take the floor of MeanTarget(height) immediately, but that is equivalent to doing
    // so only after a further division, as proven in <https://math.stackexchange.com/a/147832/185422>.
    let average_target = total_target
        / U256::from(<i64 as TryInto<u64>>::try_into(config.pow_averaging_window).unwrap());

    let expexted_bit = zcash_calculate_next_work_required(
        config,
        average_target,
        prev_block_median_time_past,
        first_block_in_interval_median_time_past,
    );

    NextWorkResult {
        expected_bits: expexted_bit,
        prev_block_median_time_past,
    }
}

fn zcash_calculate_next_work_required(
    config: &ZcashConfig,
    average_target: U256,
    last_interval_block_median_time_past: u32,
    first_interval_block_median_time_past: u32,
) -> u32 {
    let averaging_window_timespan = config.averaging_window_timespan();
    let min_actual_timespan = config.min_actual_timespan();
    let max_actual_timespan = config.max_actual_timespan();

    // Limit adjustment step
    // Use medians to prevent time-warp attacks
    let mut actual_timespan: i64 =
        (last_interval_block_median_time_past - first_interval_block_median_time_past).into();

    actual_timespan = averaging_window_timespan + (actual_timespan - averaging_window_timespan) / 4;

    if actual_timespan < min_actual_timespan {
        actual_timespan = min_actual_timespan;
    }
    if actual_timespan > max_actual_timespan {
        actual_timespan = max_actual_timespan;
    }

    // Retarget
    let new_target = average_target
        / U256::from(<i64 as TryInto<u64>>::try_into(averaging_window_timespan).unwrap());
    let (mut new_target, new_target_overflow) =
        new_target.overflowing_mul(<i64 as TryInto<u64>>::try_into(actual_timespan).unwrap());
    require!(!new_target_overflow, "new target overflow");

    if new_target > config.pow_limit {
        new_target = config.pow_limit;
    }

    new_target.target_to_bits()
}
