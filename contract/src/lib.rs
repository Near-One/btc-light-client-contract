use near_sdk::{log, near};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

use bitcoin::block::Header;

use merkle_tools;

/// Contract implementing Bitcoin light client.
/// See README.md for more details about features and implementation logic behind the code.

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
        /// Chainwork for this block (big endian storage format)
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

#[derive(Debug, BorshSerialize, BorshDeserialize)]
struct ForkState {
    fork_headers: Vec<state::Header>,
    chainwork: [u8; 32],
}

// Define the contract structure
#[near(contract_state)]
pub struct Contract {
    // fork_id -> collection of block headers from this fork
    // we will promote one of the forks if we have to do a chain reorg
    // when one of the forks will eventually reach the highest possible weight
    // fork chainwork of fork is bigger than chainwork of the main chain
    forks: near_sdk::store::LookupMap<usize, ForkState>,
    // still alive forks, others would be garbage collected
    alive_forks: std::collections::HashSet<usize>,

    // A pair of lookup maps that allows to find header by height and height by header
    height_to_header: near_sdk::store::LookupMap<usize, String>,
    header_to_height: near_sdk::store::LookupMap<String, usize>,
    // Total chainwork reached at this height
    total_chainwork_at_height: near_sdk::store::LookupMap<usize, [u8; 32]>,
    // Block with the highest chainWork, i.e., blockchain tip, you can find latest height inside of it
    heaviest_block: String,
    // Highest chainWork, i.e., accumulated PoW at current blockchain tip (big endian)
    high_score: [u8; 32],

    // Mapping of block hashes to block headers (ALL ever submitted, i.e., incl. forks)
    headers: near_sdk::store::LookupMap<String, state::Header>,

    // The latest tracked fork
    current_fork_id: usize,
}

// Define the default, which automatically initializes the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            height_to_header: near_sdk::store::LookupMap::new(b"a"),
            header_to_height: near_sdk::store::LookupMap::new(b"b"),
            total_chainwork_at_height: near_sdk::store::LookupMap::new(b"c"),
            alive_forks: std::collections::HashSet::new(),
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

    pub fn get_blockhash_by_height(&self, height: usize) -> Option<String> {
        self.height_to_header.get(&height).map(|hash| hash.to_owned())
    }

    pub fn get_height_by_blockhash(&self, blockhash: String) -> Option<usize> {
        self.header_to_height.get(&blockhash).map(|height| *height)
    }

    // TODO: To make sure we are submiting correct height we might hardcode height related to the genesis block
    // into the contract.
    pub fn submit_genesis(&mut self, block_header: Header, block_height: usize) -> bool {
        let current_block_hash= block_header.block_hash().as_raw_hash().to_string();
        let chainwork_bytes = block_header.work().to_be_bytes();

        let header = state::Header::new(block_header, chainwork_bytes, block_height);

        self.store_block_header(current_block_hash.clone(), header, chainwork_bytes);
        self.heaviest_block = current_block_hash;
        self.high_score = chainwork_bytes;
        self.total_chainwork_at_height.insert(block_height, chainwork_bytes);
        true
    }

    /// Saving block header received from a Bitcoin relay service
    /// This method is private but critically important for the overall execution flow
    fn submit_block_header(&mut self, block_header: Header) {
        // Chainwork is validated inside block_header structure (other consistency checks too)
        let prev_blockhash = block_header.prev_blockhash.to_string();
        let current_block_hash = block_header.block_hash().to_string();
        let current_block_chainwork = block_header.work();
        let chainwork_bytes = current_block_chainwork.to_be_bytes();

        log!("block: {} | chainwork: {}", current_block_hash, current_block_chainwork);

        // Computing the target height based on the previous block
        let height = 1 + self.headers
            .get(&prev_blockhash)
            .expect("cannot find prev_blockhash in headers list")
            .block_height;
        let header = state::Header::new(block_header, chainwork_bytes, height);
        let total_chainwork_on_main = bitcoin::Work::from_be_bytes(
            *self.total_chainwork_at_height.get(&(height-1)).expect("chainwork should be assign to the position")
        );

        let fork_id = self.detect_or_create_fork(&header);

        // Check if it is a MainChain or a Fork
        match fork_id {
            // Fork submission
            Some(fork_id) => {
                // Find fork
                match self.forks.get(&fork_id) {
                    Some(fork_state) => {
                        let blocks = fork_state.fork_headers.clone();
                        // Existing fork submission
                        let prev_blockheader = blocks.last().expect("ongoing fork blocks must not be empty");
                        // Validate chain
                        assert_eq!(prev_blockheader.current_blockhash, header.prev_blockhash);

                        let fork_chainwork = bitcoin::Work::from_be_bytes(fork_state.chainwork);
                        let highest_score = bitcoin::Work::from_be_bytes(self.high_score);

                        // Current chainwork is higher than on a current mainchain, let's promote the fork
                        if fork_chainwork + current_block_chainwork > highest_score {
                            // Remove the latest blocks in chain starting from fork promotion height
                            let first_fork_block = fork_state.fork_headers
                                .first()
                                .expect("first block should exist");
                            let promotion_height = first_fork_block.block_height;

                            for height_to_clean in promotion_height .. height {
                                let removed_block_header_hash = self.height_to_header.remove(&height_to_clean);

                                if let Some(hash) = removed_block_header_hash {
                                    self.header_to_height.remove(&hash);
                                }
                            }

                            let mut chainwork = bitcoin::Work::from_be_bytes(self.total_chainwork_at_height[&(height - 1)]);
                            // Update heights with block hashes from the fork
                            for block in blocks {
                                chainwork = chainwork + bitcoin::Work::from_be_bytes(block.chainwork);
                                self.store_block_header(
                                    block.current_blockhash.clone(),
                                    block.clone(),
                                    chainwork.to_be_bytes()
                                );
                                self.heaviest_block = block.current_blockhash.clone();
                                // Recalculate chainwork for every position
                                self.total_chainwork_at_height.insert(block.block_height.clone(), chainwork.to_be_bytes());
                            }

                            // Appending current block
                            chainwork = chainwork + block_header.work();
                            self.store_block_header(
                                current_block_hash.clone(),
                                header,
                                chainwork.to_be_bytes()
                            );
                            self.heaviest_block = current_block_hash;
                            // Recalculate chainwork for every position
                            self.total_chainwork_at_height.insert(height, chainwork.to_be_bytes());
                        } else {
                            // Fork still being extended: append block
                            self.store_fork_header(fork_id,
                                                   current_block_hash,
                                                   header,
                                                   (fork_chainwork + current_block_chainwork).to_be_bytes()
                            );
                        }
                    }
                    None => {
                        // Submission of new fork
                        // This should never fail
                        assert_eq!(fork_id, self.current_fork_id);
                        // Check that block is indeed a fork
                        assert_ne!(header.prev_blockhash, self.heaviest_block);

                        let height_before_fork = self.header_to_height
                            .get(&header.prev_blockhash)
                            .expect("block should be on main chain");

                        let chainwork_before_fork = self.total_chainwork_at_height
                            .get(&height_before_fork)
                            .expect("we have this height on main chain, so chainwork is also there");
                        let converted_chainwork = bitcoin::Work::from_be_bytes(*chainwork_before_fork);

                        self.store_fork_header(fork_id, current_block_hash, header, (current_block_chainwork + converted_chainwork).to_be_bytes());
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
                self.store_block_header(
                    current_block_hash,
                    header,
                    (total_chainwork_on_main + current_block_chainwork).to_be_bytes()
                );
            }
        }
    }

    /// Stores parsed block header and meta information
    fn store_block_header(&mut self, current_block_hash: String, header: state::Header, new_chainwork: [u8; 32]) {
        self.headers.insert(current_block_hash.clone(), header.clone());
        self.height_to_header.insert(header.block_height, current_block_hash.clone());
        self.header_to_height.insert(current_block_hash, header.block_height);
        self.total_chainwork_at_height.insert(header.block_height, new_chainwork);
    }

    /// Stores and handles fork submissions
    fn store_fork_header(&mut self, fork_id: usize, current_block_hash: String, header: state::Header, new_chainwork: [u8; 32]) {
        self.headers.insert(current_block_hash, header.clone());

        match self.forks.get_mut(&fork_id) {
            Some(fork_state) => {
                fork_state.fork_headers.push(header);
                fork_state.chainwork = new_chainwork;
            },
            None => {
                let new_elems = vec![(fork_id, ForkState {
                    fork_headers: vec![header].into(),
                    chainwork: new_chainwork,
                })];
                self.forks.extend(new_elems);
            }
        }
    }

    // Returns fork_id if some fork_id was found in relayer state, none if no existing fork with
    // this ID is available
    fn detect_or_create_fork(&mut self, header: &state::Header) -> Option<usize> {
        // Get latest blocks from all the fork and main
        let state = self.receive_state();

        if let Some(existing_fork_or_main_id) = state.get(&header.prev_blockhash) {
            if *existing_fork_or_main_id == 0 {
                None // Main chain, it is not a fork
            } else {
                Some(*existing_fork_or_main_id) // Some fork
            }
        } else {
            // This is a new fork, so we need to create one
            self.current_fork_id += 1;
            Some(self.current_fork_id)
        }
    }

    // Return state of the relay, so offchain service can see all the forks available
    // fork_id -> latest_block_header_hash
    pub fn receive_state(&self) -> std::collections::BTreeMap<String, usize> {
        let mut state = std::collections::BTreeMap::new();

        // Add last mainnet block to the state
        state.insert(self.heaviest_block.clone(), 0);

        //TODO: Use a set of existing forks here, we can have wholes on after doing GC
        for fork_id in 1 ..= self.current_fork_id {
            let block_hash = self.forks
                .get(&fork_id)
                .expect("fork data must be available")
                .fork_headers
                .last()
                .expect("fork data should contains at least 1 blockhash")
                .current_blockhash
                .clone();

            state.insert(block_hash, fork_id);
        }

        state
    }

    // This method return n last blocks from the mainchain
    pub fn receive_last_n_blocks(&self, n: usize, shift_from_the_end: usize) -> Vec<String> {
        let mut block_hashes = vec![];
        let tip_hash = &self.heaviest_block;
        let tip = self.headers.get(tip_hash).expect("heaviest block should be recorded");

        for height in (tip.block_height - n) .. (tip.block_height - shift_from_the_end) {
            if let Some(block_hash) = self.height_to_header.get(&height) {
                block_hashes.push(block_hash.to_string());
            }
        }

        block_hashes
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
            // block_hash: 62703463e75c025987093c6fa96e7261ac982063ea048a0550407ddbbe865345
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

    fn fork_block_header_example_3() -> Header {
        let json_value = serde_json::json!({
            // "hash": "0000000082b5015589a3fdf2d4baff403e6f0be035a5d9742c1cae6295464449",
            // "chainwork": "0000000000000000000000000000000000000000000000000000000400040004",
            "version": 1,
            "merkle_root": "999e1c837c76a1b7fbb7e57baf87b309960f5ffefbf2a9b95dd890602272f644",
            "time": 1231470173,
            "nonce": 1844305925,
            "bits": 486604799,
            "prev_blockhash": "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd",
        });
        let parsed_header = serde_json::from_value(json_value).expect("value is invalid");
        parsed_header
    }

    #[test]
    fn test_saving_mainchain_block_header() {
        let header = block_header_example();

        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_v2(header);

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

        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_v2(header);

        contract.submit_v2(fork_block_header_example());

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, state::Header::new(header,
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1],
                                                       1)
        );
    }

    #[test]
    fn test_receiving_a_latest_state_for_the_contract() {
        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_v2(block_header_example());
        contract.submit_v2(fork_block_header_example());

        let received_state = contract.receive_state();
        let expected_state: std::collections::BTreeMap<String, usize> = vec![
            (block_header_example().block_hash().to_string(), 0),
            (fork_block_header_example().block_hash().to_string(), 1)
        ].into_iter().collect();

        assert_eq!(expected_state, received_state);
    }

    // test we can insert a block and get block back by it's height
    #[test]
    fn test_getting_block_by_height() {
        let mut contract = Contract::default();
        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_v2(block_header_example());

        assert_eq!(contract.get_blockhash_by_height(0).unwrap(), genesis_block_header().block_hash().to_string());
        assert_eq!(contract.get_blockhash_by_height(1).unwrap(), block_header_example().block_hash().to_string());
    }

    #[test]
    fn test_getting_height_by_block() {
        let mut contract = Contract::default();
        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_v2(block_header_example());

        assert_eq!(contract.get_height_by_blockhash(genesis_block_header().block_hash().to_string()).unwrap(), 0);
        assert_eq!(contract.get_height_by_blockhash(block_header_example().block_hash().to_string()).unwrap(), 1);
    }

    #[test]
    fn test_submitting_existing_fork_block_header_and_promote_fork() {
        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_v2(block_header_example());

        let fork_block_header_example = fork_block_header_example();

        contract.submit_v2(fork_block_header_example);
        contract.submit_v2(fork_block_header_example_2());

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, state::Header::new(fork_block_header_example_2(),
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1],
                                                       2)
        );
    }
}
