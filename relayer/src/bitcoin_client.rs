use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::hashes::Hash;
use bitcoincore_rpc::bitcoin::BlockHash;
use bitcoincore_rpc::{jsonrpc, RpcApi};

use crate::config::Config;

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
