mod merkle_tools;

use near_sdk::{log, near};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

use bitcoin::block::Header;

/// Contract implementing Bitcoin light client

/// Bitcoin relay service can submit block headers to this service
/// Off chain relay service can request the latest block height from this service

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

    impl Into<bitcoin::block::Header> for Header {
        fn into(self) -> bitcoin::block::Header {
            let prev_blockhash_json = serde_json::json!(self.prev_blockhash);
            let merkle_root_json = serde_json::json!(self.merkle_root);

            bitcoin::block::Header {
                version: Version::from_consensus(self.version),
                prev_blockhash: serde_json::from_value(prev_blockhash_json).unwrap(),
                merkle_root: serde_json::from_value(merkle_root_json).unwrap(),
                time: self.time,
                bits: CompactTarget::from_consensus(self.bits),
                nonce: self.nonce,
            }
        }
    }
}

// Define the contract structure
#[near(contract_state)]
pub struct Contract {
    // block headers received from Bitcoin relay service
    // block_headers: Vec<state::Header>,
    // block with the highest chainWork, i.e., blockchain tip
    headers: near_sdk::store::LookupMap<usize, state::Header>,
    heaviest_block: usize
    // We use latest block to help offchain relayer to recover the reading of blocks and
    // to understand if we currently writing a fork or not.
    // latest_block_info: String
}

// Define the default, which automatically initializes the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            headers: near_sdk::store::LookupMap::new(b"d"),
            heaviest_block: 0
        }
    }
}

// Implement the contract structure
#[near]
impl Contract {
    pub fn get_last_block_header(&self) -> state::Header {
        self.headers[&self.heaviest_block].clone()
    }

    /*// We use two separates APIs to submit main_chain_block and fork_block // do we need it?
    pub fn submit_fork_block(&mut self, block_header: Header) {}
    pub fn submit_main_chain_block(&mut self, block_header: Header) {}*/

    // Saving block header received from a Bitcoin relay service
    pub fn submit_block_header(&mut self, block_header: Header, height: usize) {
        log!("Saving block_header");
        let header = state::Header::from(block_header);

        self.heaviest_block = height;
        self.headers.insert(height, header);

        // TODO: update contract to catch fork_id from realy off chain
    }

    /*
    * Verifies that a transaction is included in a block at a given block height

    * @param txid transaction identifier
    * @param txBlockHeight block height at which transacton is supposedly included
    * @param txIndex index of transaction in the block's tx merkle tree
    * @param merkleProof  merkle tree path (concatenated LE sha256 hashes) (does not contain initial transaction_hash and merkle_root)
    * @param confirmations how many confirmed blocks we want to have before the transaction is valid
    * @return True if txid is at the claimed position in the block at the given blockheight, False otherwise
    */
    pub fn verify_transaction_inclusion(
        &self,
        txid: String,
        tx_block_height: u64,
        tx_index: usize,
        merkle_proof: Vec<String>,
        confirmations: u64,
    ) -> bool {
        // check requested confirmations. No need to compute proof if insufficient confs.
        // TODO: should be block_height check here
        if (self.heaviest_block as u64).saturating_sub(tx_block_height) < confirmations {
            panic!("Not enough blocks confirmed cannot process verification");
        }

        let header = self.headers[&(tx_block_height as usize)].clone();
        let merkle_root = header.merkle_root;

        // compute merkle tree root and check if it matches block's original merkle tree root
        if merkle_tools::compute_root_from_merkle_proof(&txid, tx_index, &merkle_proof) == merkle_root {
            log!("VerityTransaction: Tx {:?} is included in block with height {}", txid, tx_block_height);
            true
        } else {
            log!("VerityTransaction: Tx {:?} is NOT included in block with height {}", txid, tx_block_height);
            false
        }
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

        contract.submit_block_header(header, 2);

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, header.into());
    }
}
