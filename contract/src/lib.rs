use std::cmp::max;
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

        /// Below, state contains additional fields not presented in the standard blockchain header
        /// those fields are used to represent additional information required for fork management
        /// and other utility functionality
        ///
        /// Current block_hash
        pub current_blockhash: String,
        /// Accumulated chainwork at this position for this block (big endian storage format)
        pub chainwork: [u8; 32],
        /// Block height in the Bitcoin network
        pub block_height: usize,
        /// Marker if this block belongs to the current mainchain or not
        pub is_main_chain: bool,
    }

    impl Header {
        pub fn new(
            header: bitcoin::block::Header,
            chainwork: [u8; 32],
            block_height: usize,
            is_main_chain: bool
        ) -> Self {
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
                is_main_chain,
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

// Reply if we cannot find a previous block in a contract state.

// Relay algorithm in this case:
// Request last 10 blocks from the mainchain.
// Move cursor back for bitcoin node and find the right block
// if still no blocks found, request 10 next old blocks from smart contract
// move cursor back on the bitcoin node
// when right block is detected, set current height to this block and start submitting them 1 by 1
// eventually this fork will be promoted on the smart contract relayer level
const NF: &str = "NF";

#[derive(Debug, BorshSerialize, BorshDeserialize)]
struct ForkState {
    fork_headers: Vec<state::Header>,
    chainwork: [u8; 32],
}

// Define the contract structure
#[near(contract_state)]
pub struct Contract {
    // A pair of lookup maps that allows to find header by height and height by header
    mainchain_height_to_header: near_sdk::store::LookupMap<usize, String>,
    mainchain_header_to_height: near_sdk::store::LookupMap<String, usize>,

    // Block with the highest chainWork, i.e., blockchain tip, you can find latest height inside of it
    mainchain_tip_blockhash: String,

    // Mapping of block hashes to block headers (ALL ever submitted, i.e., incl. forks)
    headers_pool: near_sdk::store::LookupMap<String, state::Header>,
}

// Define the default, which automatically initializes the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            mainchain_height_to_header: near_sdk::store::LookupMap::new(b"a"),
            mainchain_header_to_height: near_sdk::store::LookupMap::new(b"b"),
            headers_pool: near_sdk::store::LookupMap::new(b"h"),
            mainchain_tip_blockhash: String::new(),
        }
    }
}

// Implement the contract structure
#[near]
impl Contract {
    pub fn get_last_block_header(&self) -> state::Header {
        self.headers_pool[&self.mainchain_tip_blockhash].clone()
    }

    pub fn get_blockhash_by_height(&self, height: usize) -> Option<String> {
        self.mainchain_height_to_header.get(&height).map(|hash| hash.to_owned())
    }

    pub fn get_height_by_blockhash(&self, blockhash: String) -> Option<usize> {
        self.mainchain_header_to_height.get(&blockhash).map(|height| *height)
    }

    // TODO: To make sure we are submiting correct height we might hardcode height related to the genesis block
    // into the contract.
    pub fn submit_genesis(&mut self, block_header: Header, block_height: usize) -> bool {
        let current_block_hash= block_header.block_hash().as_raw_hash().to_string();
        let chainwork_bytes = block_header.work().to_be_bytes();

        let mut header = state::Header::new(block_header, chainwork_bytes, block_height, true);

        self.store_block_header(current_block_hash.clone(), &mut header);
        self.mainchain_tip_blockhash = current_block_hash;
        true
    }

    /// Saving block header received from a Bitcoin relay service
    /// This method is private but critically important for the overall execution flow
    fn submit_block_header(&mut self, block_header: Header) -> Result<(), usize> {
        // Chainwork is validated inside block_header structure (other consistency checks too)
        let prev_blockhash = block_header.prev_blockhash.to_string();

        let prev_block_header = if let Some(header) = self.headers_pool.get(&prev_blockhash) {
            header.clone()
        } else {
            // We do not have a previous block in the headers_pool, there is a high probability
            //it means we are starting to receive a new fork,
            // so what we do now is we are returning the error code
            // to ask the relay to deploy the previous block.
            //
            // Offchain relay now, should submit blocks one by one in decreasing height order
            // 80 -> 79 -> 78 -> ...
            // And do it until we can accept the block.
            // It means we found an initial fork position.
            // We are starting to gather new fork from this initial position.
            return Err(1);
        };

        let current_blockhash = block_header.block_hash().to_string();
        let current_block_computed_chainwork = bitcoin::Work::from_be_bytes(prev_block_header.chainwork) + block_header.work();

        // Computing the target height based on the previous block
        let height = 1 + prev_block_header.block_height;
        let total_main_chain_chainwork = bitcoin::Work::from_be_bytes(
            prev_block_header.chainwork
        );
        let mut header = state::Header::new(block_header, current_block_computed_chainwork.to_be_bytes(), height, false);

        // Main chain submission
        if prev_block_header.current_blockhash == self.mainchain_tip_blockhash {
            // Probably we should check if it is not in a mainchain?
            // chainwork > highScore
            log!("Saving to mainchain");
            // Validate chain
            assert_eq!(self.mainchain_tip_blockhash, header.prev_blockhash);

            header.is_main_chain = true;

            self.store_block_header(
                current_blockhash.clone(),
                &header
            );
            self.mainchain_tip_blockhash = current_blockhash;
        } else if prev_block_header.is_main_chain && prev_block_header.current_blockhash != self.mainchain_tip_blockhash {
            // We received a block which is connected to a mainchain block, but the mainchain block is not the last one
            // it means we are receiving a new fork submission
            self.store_fork_header(current_blockhash, header);
        } else {
            // Existing fork submission
            let fork_chainwork = bitcoin::Work::from_be_bytes(prev_block_header.chainwork);

            let main_chain_tip_header = self.headers_pool
                .get(&self.mainchain_tip_blockhash.clone())
                .expect("tip should be in a header pool");
            let main_chain_chainwork = bitcoin::Work::from_be_bytes(main_chain_tip_header.chainwork);


            self.store_fork_header(current_blockhash.clone(), header.clone());

            // Current chainwork is higher than on a current mainchain, let's promote the fork
            if fork_chainwork + current_block_computed_chainwork > total_main_chain_chainwork {
                self.reorg_chain(&current_blockhash);
            }
        }

        Ok(())
    }

    /// The most expensive operation which reorganizes the chain, based on fork weight
    fn reorg_chain(&mut self, fork_tip_header_blockhash: &str) {
        let mut header_cursor = self.headers_pool
            .get_mut(fork_tip_header_blockhash)
            .expect("fork block should be already inserted at the time");

        // Understand the length of the fork by traversing the fork from the header to the
        // top.

        // Building a vector of traversed headers

        // Comparing the size of the fork to the size of the main chain
        // starting from the fork position:
        //
        //       [m1] - [m2] - [m3] - [m4] - [m5]
        //      /
        // [m0]
        //      \
        //       [f1] - [f2] - [f3] - [f4]

        let mut fork_length = 0;
        let mut fork_blockhashes = Vec::new();

        // Iterating over fork to find where we are connected to main_chain
        while !header_cursor.is_main_chain {
            // Add current blockhash to our collection of fork blockhashes
            fork_blockhashes.push(header_cursor.current_blockhash.clone());

            // Add a marker that this block is now belongs to a new version of the main chain
            header_cursor.is_main_chain = true;

            let prev_blockhash = header_cursor.prev_blockhash.clone();

            // As we go through this iteration we are marking this fork chain as a new main
            header_cursor = self.headers_pool
                .get_mut(&prev_blockhash)
                .expect("previous fork block should be there");
        }

        // We do update operation lazily in the next fashion
        //
        //      [m1] - [m2] - [m3] - [m4] <- main_chain_tip_blockhash
        //     /
        // [m0]
        //     \
        //      [f1] - [f2] - [f3]
        //
        // What we need to do is to substitute
        //
        // 1. Substitue main chain blocks with forks when needed.
        // 2. Remove the main_chain tail.

        //      [f1] - [f2] - [f3] - [m4] <- main_chain_tip_blockhash
        //     /
        // [m0]
        //     \
        //      [f1] - [f2] - [f3]

        let first_fork_blockhash = fork_blockhashes.last().expect("fork should have at least one element");
        let start_main_chain_block_height = self.headers_pool[first_fork_blockhash].block_height;

        let last_main_chain_block_height = self.headers_pool[&self.mainchain_tip_blockhash].block_height.clone();

        // Take the longest chain
        let max_height_of_fork_or_main = max(
            start_main_chain_block_height + fork_blockhashes.len() - 1,
            last_main_chain_block_height
        );

        for height in start_main_chain_block_height ..= max_height_of_fork_or_main {
            if let Some(fork_block_blockhash) = fork_blockhashes.pop() {
                // If we have old mainchain block at the height, let's remove it
                if let Some(current_main_chain_blockhash) = self.mainchain_height_to_header.get(&height).clone() {
                    self.mainchain_header_to_height.remove(current_main_chain_blockhash);
                    self.headers_pool.remove(current_main_chain_blockhash);
                }

                self.mainchain_height_to_header.insert(height, fork_block_blockhash.clone());
                self.mainchain_header_to_height.insert(fork_block_blockhash, height);
            } else {
                // Mainchain is longer than fork (due to chainwork), let's clean the tail
                let current_main_chain_blockhash = self.mainchain_height_to_header
                    .get(&height)
                    .expect("mainchain is longer than fork, block should be there");
                self.mainchain_header_to_height.remove(current_main_chain_blockhash);
                self.headers_pool.remove(current_main_chain_blockhash);
            }
        }

        self.mainchain_tip_blockhash = fork_tip_header_blockhash.to_string();
    }

    /// Stores parsed block header and meta information
    fn store_block_header(&mut self, current_block_hash: String, header: &state::Header) {
        self.headers_pool.insert(current_block_hash.clone(), header.clone());
        self.mainchain_height_to_header.insert(header.block_height, current_block_hash.clone());
        self.mainchain_header_to_height.insert(current_block_hash, header.block_height);
    }

    /// Stores and handles fork submissions
    fn store_fork_header(&mut self, current_block_hash: String, header: state::Header) {
        self.headers_pool.insert(current_block_hash, header);
    }

    // This method return n last blocks from the mainchain
    pub fn receive_last_n_blocks(&self, n: usize, shift_from_the_end: usize) -> Vec<String> {
        let mut block_hashes = vec![];
        let tip_hash = &self.mainchain_tip_blockhash;
        let tip = self.headers_pool.get(tip_hash).expect("heaviest block should be recorded");

        for height in (tip.block_height - n) .. (tip.block_height - shift_from_the_end) {
            if let Some(block_hash) = self.mainchain_height_to_header.get(&height) {
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
        let heaviest_block_header = self.headers_pool.get(&self.mainchain_tip_blockhash).expect("heaviest block must be recorded");
        if (heaviest_block_header.block_height).saturating_sub(tx_block_height) < confirmations {
            panic!("Not enough blocks confirmed cannot process verification");
        }

        let header_hash = self.mainchain_height_to_header.get(&tx_block_height).expect("prover cannot find block by height");
        let header = self.headers_pool.get(header_hash).expect("cannot find requested transaction block");
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
        contract.submit_block_header(header).unwrap();

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, state::Header::new(header,
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 2],
                                                       1,
                                                       true)
        );
    }

    #[test]
    fn test_submitting_new_fork_block_header() {
        let header = block_header_example();

        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_block_header(header).unwrap();

        contract.submit_block_header(fork_block_header_example()).unwrap();

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, state::Header::new(header,
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 2],
                                                       1,
                                                       true)
        );
    }

    // test we can insert a block and get block back by it's height
    #[test]
    fn test_getting_block_by_height() {
        let mut contract = Contract::default();
        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_block_header(block_header_example()).unwrap();

        assert_eq!(contract.get_blockhash_by_height(0).unwrap(), genesis_block_header().block_hash().to_string());
        assert_eq!(contract.get_blockhash_by_height(1).unwrap(), block_header_example().block_hash().to_string());
    }

    #[test]
    fn test_getting_height_by_block() {
        let mut contract = Contract::default();
        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_block_header(block_header_example()).unwrap();

        assert_eq!(contract.get_height_by_blockhash(genesis_block_header().block_hash().to_string()).unwrap(), 0);
        assert_eq!(contract.get_height_by_blockhash(block_header_example().block_hash().to_string()).unwrap(), 1);
    }

    #[test]
    fn test_submitting_existing_fork_block_header_and_promote_fork() {
        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header(), 0);
        contract.submit_block_header(block_header_example()).unwrap();

        contract.submit_block_header(fork_block_header_example()).unwrap();
        contract.submit_block_header(fork_block_header_example_2()).unwrap();

        let received_header = contract.get_last_block_header();

        assert_eq!(received_header, state::Header::new(fork_block_header_example_2(),
                                                       [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 3, 0, 3],
                                                       2,
                                                       true)
        );
    }

    #[test]
    fn test_getting_an_error_if_submitting_unattached_block() {
        let mut contract = Contract::default();

        contract.submit_genesis(genesis_block_header(), 0);
        let result = contract.submit_block_header(fork_block_header_example_2());
        assert!(result.is_err());
        assert!(result.is_err_and(|value| value == 1))
    }
}
