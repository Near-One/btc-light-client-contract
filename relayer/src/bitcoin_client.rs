use crate::merkle_tools;
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

    #[allow(dead_code)]
    pub fn get_best_block_hash(&self) -> BlockHash {
        self.inner.get_best_block_hash().unwrap()
    }

    pub fn get_block_count(&self) -> u64 {
        self.inner.get_block_count().unwrap()
    }

    pub fn get_block_hash(&self, height: u64) -> BlockHash {
        self.inner.get_block_hash(height).unwrap()
    }

    pub fn get_block_header(&self, block_hash: &BlockHash) -> Header {
        self.inner.get_block_header(block_hash).unwrap()
    }

    pub fn get_block_header_by_height(&self, height: u64) -> Header {
        let block_hash = self.get_block_hash(height);
        self.get_block_header(&block_hash)
    }

    pub fn get_block(&self, block_hash: &BlockHash) -> bitcoincore_rpc::bitcoin::Block {
        self.inner.get_block(block_hash).unwrap()
    }

    pub fn get_block_by_height(&self, height: u64) -> bitcoincore_rpc::bitcoin::Block {
        let block_hash = self.get_block_hash(height);
        self.get_block(&block_hash)
    }

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
