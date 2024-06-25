use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::BlockHash;
use bitcoincore_rpc::RpcApi;

use crate::merkle_tools;

use crate::config::Config;

pub struct Client {
    inner: bitcoincore_rpc::Client,
}

impl Client {
    pub fn new(config: Config) -> Self {
        let inner = bitcoincore_rpc::Client::new(
            &config.bitcoin.endpoint,
            bitcoincore_rpc::Auth::UserPass(
                config.bitcoin.node_user.clone(),
                config.bitcoin.node_password.clone(),
            ),
        )
        .expect("failed to create a bitcoin client");

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
        &self,
        block: bitcoincore_rpc::bitcoin::Block,
        transaction_position: usize,
    ) -> Vec<String> {
        let transactions: Vec<String> = block
            .txdata
            .iter()
            .map(|tx| tx.txid().to_string())
            .collect();
        merkle_tools::merkle_proof_calculator(transactions, transaction_position)
    }
}
