use bitcoin::consensus::encode;
use bitcoin::{Transaction, TxMerkleNode};
use bitcoin::hashes::sha256d;
use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::hashes::Hash;
use bitcoincore_rpc::bitcoin::BlockHash;
use bitcoincore_rpc::{jsonrpc, RpcApi};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct AuxData {
    pub(crate) coinbase_tx: bitcoin::Transaction,
    pub(crate) parent_block_hash: BlockHash,
    pub(crate) merkle_branch: Vec<TxMerkleNode>,
    pub(crate) merkle_index: u32,
    pub(crate) chainmerkle_branch: Vec<TxMerkleNode>,
    pub(crate) chain_index: u32,
    pub(crate) parent_block: Header,
}

#[derive(Debug)]
pub struct Client {
    inner: bitcoincore_rpc::Client,
}

impl Client {
    /// Create new Bitcoin client
    ///
    /// # Panics
    /// * incorrect bitcoin endpoint
    #[must_use]
    pub fn new(config: &Config) -> Self {
        let mut builder = jsonrpc::minreq_http::Builder::new()
            .url(&config.bitcoin.endpoint)
            .unwrap();
        builder = builder.basic_auth(
            config.bitcoin.node_user.clone(),
            Some(config.bitcoin.node_password.clone()),
        );

        let inner = bitcoincore_rpc::Client::from_jsonrpc(builder.build().into());

        Self { inner }
    }

    /// Get the height of the last Bitcoin block
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    pub fn get_block_count(&self) -> Result<u64, bitcoincore_rpc::Error> {
        self.inner.get_block_count()
    }

    /// Get block hash
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    pub fn get_block_hash(&self, height: u64) -> Result<BlockHash, bitcoincore_rpc::Error> {
        self.inner.get_block_hash(height)
    }

    /// Get block header
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    pub fn get_block_header(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Header, bitcoincore_rpc::Error> {
        self.inner.get_block_header(block_hash)
    }

    pub fn get_aux_block_header(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(Header, Option<AuxData>), bitcoincore_rpc::Error> {
        let hex: String = self.inner.call("getblockheader", &[into_json(block_hash)?, false.into()])?;
        if hex.len() == 160 {
            let block1: Header = encode::deserialize_hex(&hex)?;
            return Ok((block1, None));
        } else {
            let data_bytes = hex::decode(&hex).unwrap();
            let mut cursor = 0;
            let (block1, readed_len): (Header, usize) = encode::deserialize_partial(&data_bytes).unwrap();
            cursor += readed_len;
            let (coinbase_tx, readed_len): (Transaction, usize) = encode::deserialize_partial(&data_bytes[cursor..]).unwrap();
            cursor += readed_len;
            let (parent_block_hash, reader_len): (BlockHash, usize) = encode::deserialize_partial(&data_bytes[cursor..]).unwrap();
            cursor += reader_len;
            let (merkle_branch, reader_len): (Vec<TxMerkleNode>, usize) = encode::deserialize_partial(&data_bytes[cursor..]).unwrap();
            cursor += reader_len;
            let (merkle_index, reader_len): (u32, usize) = encode::deserialize_partial(&data_bytes[cursor..]).unwrap();
            cursor += reader_len;
            let (chainmerkle_branch, reader_len): (Vec<TxMerkleNode>, usize) = encode::deserialize_partial(&data_bytes[cursor..]).unwrap();
            cursor += reader_len;
            let (chain_index, reader_len): (u32, usize) = encode::deserialize_partial(&data_bytes[cursor..]).unwrap();
            cursor += reader_len;
            let (parent_block, reader_len): (Header, usize) = encode::deserialize_partial(&data_bytes[cursor..]).unwrap();
            cursor += reader_len;

            let aux_data = AuxData {
                coinbase_tx: coinbase_tx.clone(),
                parent_block_hash,
                merkle_branch: merkle_branch.clone(),
                merkle_index,
                chainmerkle_branch: chainmerkle_branch.clone(),
                chain_index,
                parent_block
            };

            return Ok((block1, Some(aux_data)));
        }
    }

    /// Get block header by bock height
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    pub fn get_block_header_by_height(
        &self,
        height: u64,
    ) -> Result<Header, bitcoincore_rpc::Error> {
        let block_hash = self.get_block_hash(height)?;
        self.get_block_header(&block_hash)
    }

    /// Get block by block hash
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    #[allow(dead_code)]
    pub fn get_block(
        &self,
        block_hash: &BlockHash,
    ) -> Result<bitcoincore_rpc::bitcoin::Block, bitcoincore_rpc::Error> {
        self.inner.get_block(block_hash)
    }

    /// Get block by block height
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    #[allow(dead_code)]
    pub fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<bitcoincore_rpc::bitcoin::Block, bitcoincore_rpc::Error> {
        let block_hash = self.get_block_hash(height)?;
        self.get_block(&block_hash)
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn compute_merkle_proof(
        block: &bitcoincore_rpc::bitcoin::Block,
        transaction_position: usize,
    ) -> Vec<merkle_tools::H256> {
        let transactions = block
            .txdata
            .iter()
            .map(|tx| tx.compute_txid().to_byte_array().into())
            .collect();

        merkle_tools::merkle_proof_calculator(transactions, transaction_position)
    }
}

fn into_json<T>(val: T) -> Result<serde_json::Value, bitcoincore_rpc::Error>
    where
        T: serde::ser::Serialize,
{
    Ok(serde_json::to_value(val)?)
}
