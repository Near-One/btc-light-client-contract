use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::deserialize;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use crate::hash::H256;
use crate::header::Header;

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AuxData {
    pub coinbase_tx: Vec<u8>,
    pub merkle_proof: Vec<H256>,
    pub chain_merkle_proof: Vec<H256>,
    pub chain_id: usize,
    pub parent_block: Header,
}

impl AuxData {
    pub fn get_coinbase_tx(&self) -> Transaction {
        deserialize(&self.coinbase_tx).unwrap()
    }
}
