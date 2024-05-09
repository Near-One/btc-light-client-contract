// Find all our documentation at https://docs.near.org
use near_sdk::{log, near};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

use bitcoin::block::{Header, Version};
use bitcoin::CompactTarget;
use near_sdk::env::block_height;

// TODO: Idea, use bitcoin crate to handle everything in method calls, including validation and helper functions,
// TODO: but use borsh-based internal types to serialize contract state,
// TODO: state structures are stored in a special state module

mod state {
    use bitcoin::block::Version;
    use bitcoin::CompactTarget;
    use bitcoin::hashes::serde::{Deserialize, Serialize};
    use near_sdk::borsh::{BorshDeserialize, BorshSerialize};

    /// Bitcoin header to store in the block height
    #[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug, PartialEq, Clone)]
    #[serde(crate = "near_sdk::serde")]
    pub struct Header {
        /// Block version, now repurposed for soft fork signalling. (Version)
        pub version: i32,
        /// Reference to the previous block in the chain. (BlockHash)
        pub prev_blockhash: String,
        /// The root hash of the merkle tree of transactions in the block. (TxMerkleNode)
        pub merkle_root: String,
        /// The timestamp of the block, as claimed by the miner.
        pub time: u32,
        /// The target value below which the blockhash must lie. (CompactTarget)
        pub bits: u32,
        /// The nonce, selected to obtain a low enough blockhash.
        pub nonce: u32,
    }

    impl From<bitcoin::block::Header> for Header {
        fn from(header: bitcoin::block::Header) -> Self {
            Self {
                version: header.version.to_consensus(),
                prev_blockhash: header.prev_blockhash.to_string(),
                merkle_root: header.merkle_root.to_string(),
                time: header.time,
                bits: header.bits.to_consensus(),
                nonce: header.nonce,
            }
        }
    }
}

// Define the contract structure
#[near(contract_state)]
pub struct Contract {
    greeting: String,
    block_header: Vec<state::Header>,
}

// Define the default, which automatically initializes the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            block_header: Vec::new(),
            greeting: "Hello".to_string(),
        }
    }
}

// Implement the contract structure
#[near]
impl Contract {
    pub fn get_block_header(&self) -> state::Header {
        self.block_header.last().expect("genesis block should be there").clone()
    }

    // Saving block header received from a Bitcoin relay service
    pub fn submit_block_header(&mut self, block_header: Header) {
        log!("Saving block_header");

        let header = state::Header::from(block_header);

        self.block_header.push(header);
    }
}

/*
 * The rest of this file holds the inline tests for the code above
 * Learn more about Rust tests: https://doc.rust-lang.org/book/ch11-01-writing-tests.html
 */
#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::block::Header;

    // Bitcoin header example
    fn block_header_example() -> Header {
        let json_value = serde_json::json!({
            "version": 1,
            "prev_blockhash":"0000000000000000000000000000000000000000000000000000000000000000",
            "merkle_root":"4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
            "time":1231006505,
            "bits":486604799,
            "nonce":2083236893
        });
        let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
        parsed_header
    }

    #[test]
    fn test_saving_block_headers() {
        let header = block_header_example();

        let mut contract = Contract::default();

        contract.submit_block_header(header);

        let received_header = contract.get_block_header();

        assert_eq!(received_header, header.into());
    }
}
