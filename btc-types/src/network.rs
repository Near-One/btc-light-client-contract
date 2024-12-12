use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub enum Network {
    Bitcoin,
    Litecoin,
}

#[derive(Clone, Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub expected_time_secs: u64,
    pub blocks_per_adjustment: u64,
}

impl NetworkConfig {
    pub fn new(network: Network) -> Self {
        match network {
            Network::Bitcoin => NetworkConfig {
                blocks_per_adjustment: 2016,
                expected_time_secs: 2016 * 600, // blocks_per_adjustment * target_block_time_secs,
            },
            Network::Litecoin => NetworkConfig {
                blocks_per_adjustment: 2016,
                expected_time_secs: 2016 * 150,
            },
        }
    }
}
