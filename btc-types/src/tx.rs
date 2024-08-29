use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use crate::hash::H256;

//#[derive(Clone, Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct Transaction {
    pub version: u32,
    pub lock_time: u32,
    pub input: Vec<TxIn>,
    pub output: Vec<TxOut>,
}

pub struct TxIn {
    pub previous_tx_hash: H256,
    pub previous_output_index: u32,
    pub script_sig: Vec<u8>,
    pub sequence: u32,
}

pub struct TxOut {
    pub value: u64,
    pub script_pub_key: Vec<u8>,
}

/*impl TryFrom<Vec<u8>> for H256 {
    type Error = &'static str;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Ok(H256(value.try_into().map_err(|_| "Invalid hex length")?))
    }
}*/
