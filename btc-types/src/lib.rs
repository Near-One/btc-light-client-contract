mod u256;
use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
pub use u256::U256;
pub type Target = U256;
pub type Work = U256;
pub type ChainWork = H256;

#[derive(
    BorshDeserialize,
    BorshSerialize,
    Serialize,
    Deserialize,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Debug,
    Default,
)]
pub struct H256(pub [u8; 32]);

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
pub struct Header {
    /// Block version, now repurposed for soft fork signalling.
    pub version: i32,
    /// Reference to the previous block in the chain.
    pub prev_block_hash: H256,
    /// The root hash of the merkle tree of transactions in the block.
    pub merkle_root: H256,
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
    /// Current `block_hash`
    pub current_block_hash: H256,
    /// Accumulated chainwork at this position for this block (big endian storage format)
    pub chainwork: ChainWork,
    /// Block height in the Bitcoin network
    pub block_height: u64,
}

impl Header {
    fn double_sha256(data: &[u8]) -> H256 {
        let res: [u8; 32] = near_sdk::env::sha256(data).try_into().unwrap();
        H256(near_sdk::env::sha256(&res).try_into().unwrap())
    }

    pub fn target(&self) -> Target {
        self.bits.into()
    }

    pub fn work(&self) -> Work {
        self.target().inverse()
    }

    pub fn block_hash(&self) -> H256 {
        let mut block_header = Vec::new();
        block_header.extend_from_slice(&self.version.to_le_bytes());
        block_header.extend(self.prev_block_hash.0.iter().rev());
        block_header.extend(self.merkle_root.0.iter().rev());
        block_header.extend_from_slice(&self.time.to_le_bytes());
        block_header.extend_from_slice(&self.bits.to_be_bytes());
        block_header.extend_from_slice(&self.nonce.to_le_bytes());

        Self::double_sha256(&block_header)
    }
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
pub struct ExtendedHeader {
    pub block_header: Header,
    /// Below, state contains additional fields not presented in the standard blockchain header
    /// those fields are used to represent additional information required for fork management
    /// and other utility functionality
    ///
    /// Current `block_hash`
    pub current_block_hash: H256,
    /// Accumulated chainwork at this position for this block (big endian storage format)
    pub chainwork: [u8; 32],
    /// Block height in the Bitcoin network
    pub block_height: u64,
}
