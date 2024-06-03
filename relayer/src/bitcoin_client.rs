use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::hashes::{sha256d, Hash};
use bitcoincore_rpc::bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::BlockHash;
use bitcoincore_rpc::bitcoin::{Transaction, TxMerkleNode};
use bitcoincore_rpc::{RawTx, RpcApi};
use rs_merkle::algorithms::Sha256;

use crate::merkle_tools;

use crate::config::Config;

pub struct Client {
    config: Config,
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

        Self { config, inner }
    }

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

    pub fn compute_merkle_proof_2(
        &self,
        block: bitcoincore_rpc::bitcoin::Block,
        root: TxMerkleNode,
    ) -> Vec<u8> {
        use rs_merkle::proof_serializers;
        use rs_merkle::MerkleProof;
        use rs_merkle::MerkleTree;

        for just_transaction in block.txdata.iter() {
            println!("hex trans: {:?}", just_transaction.txid());
        }

        let mut leaves: Vec<[u8; 32]> = vec![];

        for transaction in block.txdata {
            leaves.push(transaction.txid().to_raw_hash().as_byte_array().clone());
        }

        for trans in leaves.iter() {
            println!("raw transaction: {:?}", trans);
        }

        let merkle_tree = MerkleTree::<Sha256d>::from_leaves(&leaves);

        // Choosing an index of the transaction we want to proof
        // Constructing Merkle Proof
        let indices_to_prove = vec![0];
        let merkle_proof = merkle_tree.proof(&indices_to_prove);

        // Serialize proof to pass it to the client over the network
        let proof_bytes = merkle_proof.serialize::<proof_serializers::DirectHashesOrder>();

        // Deserializing the proof
        let proof_result = MerkleProof::<Sha256d>::from_bytes(proof_bytes.as_slice()).unwrap();

        let first_hash = proof_result.proof_hashes().first();
        let root_hash = proof_result.proof_hashes().last();

        for proof in proof_result.proof_hashes_hex() {
            println!("hash: {:?}", proof)
        }
        println!("MY MERKLE ROOT: {:?}", merkle_tree.root().unwrap().as_hex());
        println!("MERKLE ROOT: {:?}", root.as_raw_hash());

        return proof_bytes;
    }
}

use rs_merkle::Hasher;

#[derive(Clone, Debug)]
struct Sha256d;

impl Hasher for Sha256d {
    type Hash = [u8; 32];

    fn hash(data: &[u8]) -> Self::Hash {
        let first = Sha256::hash(data);
        let second = Sha256::hash(&first);
        second
    }
}
