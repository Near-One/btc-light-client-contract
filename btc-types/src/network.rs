use near_sdk::near;

use crate::u256::U256;

pub const ZCASH_MEDIAN_TIME_SPAN: usize = 11;

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug)]
pub enum Network {
    Mainnet,
    Testnet,
}

pub fn get_bitcoin_config(network: Network) -> NetworkConfig {
    match network {
        Network::Mainnet => NetworkConfig {
            blocks_per_adjustment: 2016,
            expected_time_secs: 2016 * 600, // blocks_per_adjustment * target_block_time_secs,
            proof_of_work_limit_bits: 0x1d00ffff,
            pow_target_time_between_blocks_secs: 600, // 10 minutes
            pow_allow_min_difficulty_blocks: false,
            pow_limit: U256::new(
                0x0000_0000_ffff_ffff_ffff_ffff_ffff_ffff,
                0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            ),
        },
        Network::Testnet => NetworkConfig {
            blocks_per_adjustment: 2016,
            expected_time_secs: 2016 * 600, // blocks_per_adjustment * target_block_time_secs,
            proof_of_work_limit_bits: 0x1d00ffff,
            pow_target_time_between_blocks_secs: 600, // 10 minutes
            pow_allow_min_difficulty_blocks: true,
            pow_limit: U256::new(
                0x0000_0000_ffff_ffff_ffff_ffff_ffff_ffff,
                0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            ),
        },
    }
}

pub fn get_litecoin_config(network: Network) -> NetworkConfig {
    match network {
        Network::Mainnet => NetworkConfig {
            blocks_per_adjustment: 2016,
            expected_time_secs: 2016 * 150,
            proof_of_work_limit_bits: 0x1e0fffff,
            pow_target_time_between_blocks_secs: 150, // 2.5 minutes
            pow_allow_min_difficulty_blocks: false,
            pow_limit: U256::new(
                0x0000_0fff_ffff_ffff_ffff_ffff_ffff_ffff,
                0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            ),
        },
        Network::Testnet => NetworkConfig {
            blocks_per_adjustment: 2016,
            expected_time_secs: 2016 * 150,
            proof_of_work_limit_bits: 0x1e0fffff,
            pow_target_time_between_blocks_secs: 150, // 2.5 minutes
            pow_allow_min_difficulty_blocks: true,
            pow_limit: U256::new(
                0x0000_0fff_ffff_ffff_ffff_ffff_ffff_ffff,
                0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            ),
        },
    }
}

pub fn get_zcash_config(network: Network) -> ZcashConfig {
    match network {
        Network::Mainnet => ZcashConfig {
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L288
            proof_of_work_limit_bits: 0x1f07ffff,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L103
            pow_limit: U256::new(
                0x0007_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
                0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            ),
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L104
            pow_averaging_window: 17,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L127
            blossom_activation_height: 653600,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.h#L243
            pre_blossom_pow_target_spacing: 150,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.h#L244
            post_blossom_pow_target_spacing: 75,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L429
            pow_max_adjust_down: 32, // 32% adjustment down
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L430
            pow_max_adjust_up: 16, // 16% adjustment up
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L110
            pow_allow_min_difficulty_blocks_after_height: None,
        },
        Network::Testnet => ZcashConfig {
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L629
            proof_of_work_limit_bits: 0x2007ffff,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L426
            pow_limit: U256::new(
                0x07ff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
                0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            ),
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L427
            pow_averaging_window: 17,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L450
            blossom_activation_height: 584000,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.h#L243
            pre_blossom_pow_target_spacing: 150,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.h#L244
            post_blossom_pow_target_spacing: 75,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L429
            pow_max_adjust_down: 32,
            //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L430
            pow_max_adjust_up: 16,
            // https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/chainparams.cpp#L433
            pow_allow_min_difficulty_blocks_after_height: Some(299187),
        },
    }
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug)]
pub struct NetworkConfig {
    pub expected_time_secs: u64,
    pub blocks_per_adjustment: u64,
    pub proof_of_work_limit_bits: u32,
    pub pow_target_time_between_blocks_secs: u32,
    pub pow_allow_min_difficulty_blocks: bool,
    pub pow_limit: U256,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug)]
pub struct ZcashConfig {
    pub proof_of_work_limit_bits: u32,
    pub pow_limit: U256,
    pub pow_averaging_window: i64,
    pub blossom_activation_height: u64,
    pub pre_blossom_pow_target_spacing: i64,
    pub post_blossom_pow_target_spacing: i64,
    pub pow_max_adjust_down: i64,
    pub pow_max_adjust_up: i64,
    pub pow_allow_min_difficulty_blocks_after_height: Option<u64>,
}

impl ZcashConfig {
    //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.cpp#L397
    pub fn pow_target_spacing(&self, height: u64) -> i64 {
        let blossom_active = height >= self.blossom_activation_height;
        if blossom_active {
            self.post_blossom_pow_target_spacing
        } else {
            self.pre_blossom_pow_target_spacing
        }
    }

    //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.cpp#L406
    pub fn averaging_window_timespan(&self, height: u64) -> i64 {
        self.pow_averaging_window * self.pow_target_spacing(height)
    }

    //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.cpp#L410
    pub fn min_actual_timespan(&self, height: u64) -> i64 {
        (self.averaging_window_timespan(height) * (100 - self.pow_max_adjust_up)) / 100
    }

    //https://github.com/zcash/zcash/blob/2352fbc1ed650ac4369006bea11f7f20ee046b84/src/consensus/params.cpp#L414
    pub fn max_actual_timespan(&self, height: u64) -> i64 {
        (self.averaging_window_timespan(height) * (100 + self.pow_max_adjust_down)) / 100
    }
}
