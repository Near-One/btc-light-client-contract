// Find all our documentation at https://docs.near.org
use near_sdk::{log, near};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::store::key::Sha256;

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
    block_header: Vec<state::Header>, // block headers received from Bitcoin relay service
    headers: near_sdk::store::UnorderedMap<String, state::Header>, // mapping of block height to header
    heaviest_block: String // block with the highest chainWork, i.e., blockchain tip
}

// Define the default, which automatically initializes the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            block_header: Vec::new(),
            headers: near_sdk::store::UnorderedMap::new(b"d"),
            heaviest_block: "".to_string()
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
        block_header.merkle_root.to_string();
        let header = state::Header::from(block_header);
        self.block_header.push(header);
    }

    pub fn verify_tx(
        &self,
        txid: [u8; 32],
        tx_block_height: u64,
        tx_index: u64,
        merkle_proof: &[u8],
        confirmations: u64,
    ) -> bool {
        // txid must not be 0
        if txid == [0; 32] {
            panic!("ERR_INVALID_TXID");
        }

        // check requested confirmations. No need to compute proof if insufficient confs.
        // TODO: should be block_height check here
        if self.headers[&self.heaviest_block].nonce as u64 - tx_block_height < confirmations {
            panic!("ERR_CONFIRMS");
        }

        // TODO: change storage layout again
        // access local state to get the right block header hash
        let header: Header = self.block_header[tx_block_height as usize].into();
        let merkle_root = header.merkle_root;

        // Check merkle proof structure: 1st hash == txid and last hash == merkle_root
        if &merkle_proof[0..32] != txid {
            panic!("ERR_MERKLE_PROOF");
        }
        if &merkle_proof[merkle_proof.len() - 32..] != merkle_root {
            panic!("ERR_MERKLE_PROOF");
        }

        // compute merkle tree root and check if it matches block's original merkle tree root
        if Self::compute_merkle(&txid, tx_index, merkle_proof) == merkle_root {
            // emit VerityTransaction event
            println!("VerityTransaction: {:?}, {}", txid, tx_block_height);
            return true;
        }

        false
    }

    fn compute_merkle(tx_hash: &[u8; 32], tx_index: u64, merkle_proof: &[u8]) -> [u8; 32] {
        // Special case: only coinbase tx in block. Root == proof
        if merkle_proof.len() == 32 {
            return merkle_proof.try_into().expect("Invalid merkle proof length");
        }

        // Merkle proof length must be greater than 64 and power of 2. Case length == 32 covered above.
        if merkle_proof.len() <= 64 || (merkle_proof.len() & (merkle_proof.len() - 1)) != 0 {
            panic!("ERR_MERKLE_PROOF");
        }

        let mut result_hash = *tx_hash;
        let mut index = tx_index;

        for i in 1..merkle_proof.len() / 32 {
            let hash_slice = &merkle_proof[i * 32..(i + 1) * 32];

            if index % 2 == 1 {
                result_hash = Self::concat_sha256_hash(hash_slice, &result_hash);
            } else {
                result_hash = Self::concat_sha256_hash(&result_hash, hash_slice);
            }
            index /= 2;
        }

        result_hash
    }

    fn concat_sha256_hash(left: &[u8], right: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(left);
        hasher.update(right);
        hasher.finalize().into()
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
