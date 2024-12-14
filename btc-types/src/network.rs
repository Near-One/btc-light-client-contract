use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub enum Network {
    Bitcoin,
    BitcoinTestnet,
    Litecoin,
    LitecoinTestnet,
}

#[derive(Copy, Clone, Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub expected_time_secs: u64,
    pub blocks_per_adjustment: u64,
    pub proof_of_work_limit_bits: u32,
    pub pow_target_time_between_blocks_secs: u32,
    pub pow_allow_min_difficulty_blocks: bool,
}

impl NetworkConfig {
    pub fn new(network: Network) -> Self {
        match network {
            Network::Bitcoin => NetworkConfig {
                blocks_per_adjustment: 2016,
                expected_time_secs: 2016 * 600, // blocks_per_adjustment * target_block_time_secs,
                proof_of_work_limit_bits: 0x1d00ffff,
                pow_target_time_between_blocks_secs: 600, // 10 minutes
                pow_allow_min_difficulty_blocks: false,
            },
            Network::BitcoinTestnet => NetworkConfig {
                blocks_per_adjustment: 2016,
                expected_time_secs: 2016 * 600, // blocks_per_adjustment * target_block_time_secs,
                proof_of_work_limit_bits: 0x1d00ffff,
                pow_target_time_between_blocks_secs: 600, // 10 minutes
                pow_allow_min_difficulty_blocks: true,
            },
            Network::Litecoin => NetworkConfig {
                blocks_per_adjustment: 2016,
                expected_time_secs: 2016 * 150,
                proof_of_work_limit_bits: 0x1e0fffff,
                pow_target_time_between_blocks_secs: 150, // 2.5 minutes
                pow_allow_min_difficulty_blocks: false,
            },
            Network::LitecoinTestnet => NetworkConfig {
                blocks_per_adjustment: 2016,
                expected_time_secs: 2016 * 150,
                proof_of_work_limit_bits: 0x1e0fffff,
                pow_target_time_between_blocks_secs: 150, // 2.5 minutes
                pow_allow_min_difficulty_blocks: true,
            },
        }
    }
}
