use near_sdk::{log, near};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

use bitcoin::block::Header;

use merkle_tools;

/// Contract implementing Bitcoin light client. See README.md for more details about features
/// and implemetation logic behind the code.

/// This contract could work in a pairing with an external off-chain relay service. To learn more about
/// relay, take a look at the relay service documentation.

mod state {
    use bitcoin::block::Version;
    use bitcoin::CompactTarget;
    use bitcoin::hashes::serde::{Deserialize, Serialize};
    use near_sdk::borsh::{BorshDeserialize, BorshSerialize};

    /// Bitcoin header to store in the block height
    #[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug, PartialEq, Clone)]
    #[serde(crate = "near_sdk::serde")]
    pub struct Header {
        /// Block version, now repurposed for soft fork signalling.
        pub version: i32,
        /// Current block_hash
        pub current_blockhash: String,
        /// Reference to the previous block in the chain.
        pub prev_blockhash: String,
        /// The root hash of the merkle tree of transactions in the block.
        pub merkle_root: String,
        /// The timestamp of the block, as claimed by the miner.
        pub time: u32,
        /// The target value below which the blockhash must lie.
        pub bits: u32,
        /// The nonce, selected to obtain a low enough blockhash.
        pub nonce: u32,
        /// Chainwork for this block
        pub chainwork: [u8; 32],
        /// Block height in the Bitcoin network
        pub block_height: usize,
    }

    impl Header {
        pub fn new(header: bitcoin::block::Header, chainwork: [u8; 32], block_height: usize) -> Self {
            Self {
                version: header.version.to_consensus(),
                current_blockhash: header.block_hash().to_string(),
                prev_blockhash: header.prev_blockhash.to_string(),
                merkle_root: header.merkle_root.to_string(),
                time: header.time,
                bits: header.bits.to_consensus(),
                nonce: header.nonce,
                chainwork: chainwork,
                block_height: block_height,
            }
        }

        pub fn to_bitcoin_block_header(&self) -> bitcoin::block::Header {
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

struct ForkState {
    fork_chain: Vec<state::Header>,
    heaviest_block: usize,
    chainwork: usize,
}

// Define the contract structure
#[near(contract_state)]
pub struct Contract {
    // fork_id -> collection of block headers from this fork
    // we will promote one of the forks if we have to do a chain reorg
    // when one of the forks will eventually reach the highest possible weight
    // fork chainwork of fork is bigger than chainwork of the main chain
    forks: near_sdk::store::LookupMap<usize, Vec<state::Header>>,

    // mapping(bytes32 => HeaderInfo) public _headers;
    // mapping(uint256 => bytes32) public _mainChain; // mapping of block heights to block hashes of the MAIN CHAIN

    height_to_header: near_sdk::store::LookupMap<usize, String>,

    // Mapping of block hashes to block headers (ALL ever submitted, i.e., incl. forks)
    headers: near_sdk::store::LookupMap<String, state::Header>,

    // The latest tracked fork
    current_fork_id: usize,

    // Block with the highest chainWork, i.e., blockchain tip
    heaviest_block: String,

    // Highest chainWork, i.e., accumulated PoW at current blockchain tip
    high_score: [u8; 32]
}

// Define the default, which automatically initializes the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            height_to_header: near_sdk::store::LookupMap::new(b"a"),
            headers: near_sdk::store::LookupMap::new(b"h"),
            forks: near_sdk::store::LookupMap::new(b"f"),
            current_fork_id: 0,
            heaviest_block: String::new(),
            high_score: [0; 32],
        }
    }
}

// Implement the contract structure
#[near]
impl Contract {
    pub fn get_last_block_header(&self) -> state::Header {
        self.headers[&self.heaviest_block].clone()
    }

    // TODO: Should we submit genesis block separately or we can try to find the way to do it as a part
    // TODO: of a general flow?
    pub fn submit_genesis(&mut self, block_header: Header) -> bool {
        let current_block_hash= block_header.block_hash().as_raw_hash().to_string();
        let chainwork_bytes = block_header.work().to_be_bytes();
        let height = 0;

        let header = state::Header::new(block_header, chainwork_bytes, height);

        self.store_block_header(current_block_hash.clone(), header);
        self.heaviest_block = current_block_hash;
        true
    }

    // Submit fork headers for a new fork
    pub fn submit_new_fork_header(&mut self, block_header: Header, height: usize) -> bool {
        self.current_fork_id = self.current_fork_id + 1;
        self.submit_block_header(block_header, Some(self.current_fork_id), height);
        true
    }

    // Submit main chain headers
    pub fn submit_main_chain_header(&mut self, block_header: Header, height: usize) -> bool {
        self.submit_block_header(block_header, None, height);
        true
    }

    // Submit fork headers
    pub fn submit_fork_header(&mut self, block_header: Header, height: usize, fork_id: usize) -> bool {
        self.submit_block_header(block_header, Some(fork_id), height);
        true
    }

    /// Saving block header received from a Bitcoin relay service
    /// This method is private but critically important for the overall execution flow
    fn submit_block_header(&mut self, block_header: Header, fork_id: Option<usize>, height: usize) {
        // Chainwork is validated inside block_header structure (other consistency checks too)
        let prev_blockhash = block_header.prev_blockhash.to_string();
        let current_block_hash = block_header.block_hash().to_string();
        let chainwork = block_header.work();
        let chainwork_bytes = chainwork.to_be_bytes();
        log!("block: {} | chainwork: {}", current_block_hash, chainwork);

        // Checking that previous block exists on the chain, abort if not
        if self.headers.get(&prev_blockhash).is_none() {
            panic!("Cannot find prev_blockhash in header list");
        }

        let header = state::Header::new(block_header, chainwork_bytes, height);

        // Check if it is a MainChain or a Fork
        match fork_id {
            // Fork submission
            Some(fork_id) => {
                // Find fork
                match self.forks.get(&fork_id) {
                    Some(blocks) => {
                        // Existing fork submission
                        let prev_blockheader = blocks.last().expect("ongoing fork blocks must not be empty");
                        // Validate chain
                        assert_eq!(prev_blockheader.current_blockhash, header.prev_blockhash);

                        // Current chainwork is higher than on a current mainchain, let's promote the fork
                        if chainwork_bytes > self.high_score {
                            // Remove the latest blocks in chain starting from fork promotion height
                            let first_fork_block = blocks.first().expect("first block should exist");
                            let promotion_height = first_fork_block.block_height;
                            for height_to_clean in promotion_height .. height {
                                self.height_to_header.remove(&height_to_clean);
                            }

                            // Update heights with block hashes from the fork
                            for block in blocks {
                                self.height_to_header.insert(block.block_height, block.current_blockhash.clone());
                            }
                        } else {
                            // Fork still being extended: append block
                            self.store_fork_header(fork_id, current_block_hash, header);
                        }
                    }
                    None => {
                        // Submission of new fork
                        // This should never fail
                        assert_eq!(fork_id, self.current_fork_id);
                        // Check that block is indeed a fork
                        assert_ne!(header.prev_blockhash, self.heaviest_block);

                        self.store_fork_header(fork_id, current_block_hash, header);
                    }
                }
            },
            // Mainchain submission
            None => {
                // Probably we should check if it is not in a mainchain?
                // chainwork > highScore
                log!("Saving to mainchain");
                // Validate chain
                assert_eq!(self.heaviest_block, header.prev_blockhash);

                self.heaviest_block = current_block_hash.clone();
                self.high_score = chainwork_bytes;
                self.store_block_header(current_block_hash, header);
            }
        }
    }

    /// Stores parsed block header and meta information
    fn store_block_header(&mut self, current_block_hash: String, header: state::Header) {
        self.headers.insert(current_block_hash.clone(), header.clone());
        self.height_to_header.insert(header.block_height, current_block_hash);
    }

    /// Stores and handles fork submissions
    fn store_fork_header(&mut self, fork_id: usize, current_block_hash: String, header: state::Header) {
        self.headers.insert(current_block_hash, header.clone());

        match self.forks.get(&fork_id) {
            Some(blocks) => {
                let mut copy = blocks.clone();
                copy.push(header);

                self.forks.insert(fork_id, copy);
            },
            None => {
                let new_elems = vec![(fork_id, vec![header])];
                self.forks.extend(new_elems);
            }
        }
    }

    // Return state of the relay, so offchain service can see all the forks available
    // fork_id -> latest_block_header_hash
    pub fn receive_state(&self) -> std::collections::HashMap<usize, String> {
        let mut state = std::collections::HashMap::new();

        // Add last mainnet block to the state
        state.insert(0, self.heaviest_block.clone());

        // Extract all of the last fork blocks
        for fork_id in 1 ..= self.current_fork_id {
            let block_hash = self.forks
                .get(&fork_id)
                .expect("fork data must be available")
                .last()
                .expect("fork data should contains at least 1 blockhash")
                .current_blockhash
                .clone();

            state.insert(fork_id, block_hash);
        }

        state
    }


    /// Verifies that a transaction is included in a block at a given block height

    /// @param txid transaction identifier
    /// @param txBlockHeight block height at which transacton is supposedly included
    /// @param txIndex index of transaction in the block's tx merkle tree
    /// @param merkleProof  merkle tree path (concatenated LE sha256 hashes) (does not contain initial transaction_hash and merkle_root)
    /// @param confirmations how many confirmed blocks we want to have before the transaction is valid
    /// @return True if txid is at the claimed position in the block at the given blockheight, False otherwise
    pub fn verify_transaction_inclusion(
        &self,
        txid: String,
        tx_block_height: usize,
        tx_index: usize,
        merkle_proof: Vec<String>,
        confirmations: usize,
    ) -> bool {
        // check requested confirmations. No need to compute proof if insufficient confs.
        let heaviest_block_header = self.headers.get(&self.heaviest_block).expect("heaviest block must be recorded");
        if (heaviest_block_header.block_height).saturating_sub(tx_block_height) < confirmations {
            panic!("Not enough blocks confirmed cannot process verification");
        }

        let header_hash = self.height_to_header.get(&tx_block_height).expect("prover cannot find block by height");
        let header = self.headers.get(header_hash).expect("cannot find requested transaction block");
        let merkle_root = header.clone().merkle_root;

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
    use bitcoin::hex::DisplayHex;

    fn genesis_block_header() -> Header {
        let json_value = serde_json::json!({
            "version": 1,
            "prev_blockhash": "0000000000000000000000000000000000000000000000000000000000000000",
            "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
            "time": 1231006505,
            "bits": 486604799,
            "nonce": 2083236893
        });
        let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
        parsed_header
    }

    // Bitcoin header example
    fn block_header_example() -> Header {
        let json_value = serde_json::json!({
            "version": 1,
            "prev_blockhash": "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
            "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
            "time": 1231006506,
            "bits": 486604799,
            "nonce": 2083236893
        });
        let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
        parsed_header
    }

    fn fork_block_header_example() -> Header {
        let json_value = serde_json::json!({
            // "hash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
            //"chainwork": "0000000000000000000000000000000000000000000000000000000200020002",
            "version": 1,
            "merkle_root": "0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098",
            "time": 1231469665,
            "nonce": 2573394689_u32,
            "bits": 486604799,
            "prev_blockhash": "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
        });
        let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
        parsed_header
    }

    fn fork_block_header_example_2() -> Header {
        let json_value = serde_json::json!({
            // "hash": "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd",
            // "chainwork": "0000000000000000000000000000000000000000000000000000000300030003",
          "version": 1,
          "merkle_root": "9b0fc92260312ce44e74ef369f5c66bbb85848f2eddd5a7a1cde251e54ccfdd5",
          "time": 1231469744,
          "nonce": 1639830024,
          "bits": 486604799,
          "prev_blockhash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
        });
        let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
        parsed_header
    }

    #[test]
    fn test_saving_mainchain_block_header() {
        let header = block_header_example();

        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header());
        contract.submit_block_header(header, None, 1);

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, state::Header::new(header,
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1],
                                                       1)
        );
    }

    #[test]
    fn test_submitting_new_fork_block_header() {
        let header = block_header_example();

        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header());
        contract.submit_block_header(header, None, 1);

        contract.submit_new_fork_header(fork_block_header_example(), 1);

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, state::Header::new(header,
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1],
                                                       1)
        );
    }

    #[test]
    fn test_receiving_a_latest_state_for_the_contract() {
        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header());
        contract.submit_block_header(block_header_example(), None, 1);
        contract.submit_new_fork_header(fork_block_header_example(), 1);

        let received_state = contract.receive_state();
        let expected_state: std::collections::HashMap<usize, String> = vec![
            (0, block_header_example().block_hash().to_string()),
            (1, fork_block_header_example().block_hash().to_string())
        ].into_iter().collect();

        assert_eq!(expected_state, received_state);
    }

    // TODO: Modify this test to properly check fork promotion
    #[test]
    fn test_submitting_existing_fork_block_header() {
        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header());
        contract.submit_main_chain_header(block_header_example(), 1);

        let fork_block_header_example = fork_block_header_example();

        contract.submit_new_fork_header(fork_block_header_example, 1);
        contract.submit_fork_header(fork_block_header_example_2(), 2, 1);

        let received_header = contract.get_last_block_header();

        // TODO: Demonstrate fork promotio in this test
        assert_eq!(received_header, state::Header::new(fork_block_header_example_2(),
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1],
                                                       1)
        );
    }
}
