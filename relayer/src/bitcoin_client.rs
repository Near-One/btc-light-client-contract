#[cfg(not(feature = "zcash"))]
use bitcoin::consensus::{encode, serialize};
use bitcoin::TxMerkleNode;
#[cfg(not(feature = "zcash"))]
use bitcoin::Transaction;
#[cfg(not(feature = "zcash"))]
use bitcoincore_rpc::bitcoin::block::Header as BitcoinHeader;
use bitcoincore_rpc::bitcoin::hashes::Hash;
use bitcoincore_rpc::bitcoin::BlockHash;
use bitcoincore_rpc::jsonrpc::minreq_http::HttpError;
use bitcoincore_rpc::jsonrpc::Transport;
use bitcoincore_rpc::{jsonrpc, RpcApi};
use btc_types::header::Header;
use jsonrpc::{Request, Response};
use std::error::Error;

use crate::config::Config;

#[allow(dead_code)]
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

struct CustomMinreqHttpTransport {
    url: String,
    timeout: std::time::Duration,
    basic_auth: Option<String>,
    headers: Vec<(String, String)>,
}

impl CustomMinreqHttpTransport {
    fn request<R>(&self, req: impl serde::Serialize) -> Result<R, jsonrpc::minreq_http::Error>
    where
        R: for<'a> serde::de::Deserialize<'a>,
    {
        let req = match &self.basic_auth {
            Some(auth) => minreq::Request::new(minreq::Method::Post, &self.url)
                .with_timeout(self.timeout.as_secs())
                .with_header("Authorization", auth)
                .with_headers(self.headers.clone())
                .with_json(&req)?,
            None => minreq::Request::new(minreq::Method::Post, &self.url)
                .with_timeout(self.timeout.as_secs())
                .with_json(&req)?,
        };

        // Send the request and parse the response. If the response is an error that does not
        // contain valid JSON in its body (for instance if the bitcoind HTTP server work queue
        // depth is exceeded), return the raw HTTP error so users can match against it.
        let resp = req.send()?;
        match resp.json() {
            Ok(json) => Ok(json),
            Err(minreq_err) => {
                if resp.status_code == 200 {
                    Err(jsonrpc::minreq_http::Error::Minreq(minreq_err))
                } else {
                    Err(jsonrpc::minreq_http::Error::Http(HttpError {
                        status_code: resp.status_code,
                        body: resp.as_str().unwrap_or("").to_string(),
                    }))
                }
            }
        }
    }

    pub fn basic_auth(user: String, pass: Option<&str>) -> String {
        let mut s = user;
        s.push(':');
        if let Some(ref pass) = pass {
            s.push_str(pass.as_ref());
        }
        format!("Basic {}", &jsonrpc::base64::encode(s.as_bytes()))
    }
}

impl Transport for CustomMinreqHttpTransport {
    fn send_request(&self, req: Request) -> Result<Response, jsonrpc::Error> {
        Ok(self.request(req)?)
    }

    fn send_batch(&self, reqs: &[Request]) -> Result<Vec<Response>, jsonrpc::Error> {
        Ok(self.request(reqs)?)
    }

    fn fmt_target(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.url)
    }
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
        let config = config.bitcoin.clone();

        let client = CustomMinreqHttpTransport {
            url: config.endpoint,
            timeout: std::time::Duration::from_secs(15),
            basic_auth: Some(CustomMinreqHttpTransport::basic_auth(
                config.node_user,
                Some(&config.node_password),
            )),
            headers: config.node_headers.unwrap_or_default(),
        };

        let inner = bitcoincore_rpc::Client::from_jsonrpc(client.into());

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
    #[allow(dead_code)]
    pub fn get_block_header(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Header, Box<dyn std::error::Error + Send + Sync>> {
        let hex: String = self.inner.call(
            "getblockheader",
            &[serde_json::to_value(block_hash)?, false.into()],
        )?;
        let decoded_hex = hex::decode(hex)?;
        Ok(Header::from_block_header_vec(&decoded_hex)?)
    }

    #[cfg(feature = "zcash")]
    pub fn get_aux_block_header(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(Header, Option<AuxData>), Box<dyn Error + Send + Sync>> {
        Ok((self.get_block_header(block_hash)?, None))
    }

    /// Get aux block header
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    #[cfg(not(feature = "zcash"))]
    pub fn get_aux_block_header(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(Header, Option<AuxData>), Box<dyn Error + Send + Sync>> {
        let hex: String = self
            .inner
            .call("getblockheader", &[into_json(block_hash)?, false.into()])?;
        if hex.len() == 160 {
            let decoded_hex = hex::decode(hex)?;
            let block1: Header = Header::from_block_header_vec(&decoded_hex)?;
            return Ok((block1, None));
        }
        let data_bytes = hex::decode(&hex)?;
        let mut cursor = 0;
        let (block1, readed_len): (BitcoinHeader, usize) =
            encode::deserialize_partial(&data_bytes)?;
        cursor += readed_len;
        let (coinbase_tx, readed_len): (Transaction, usize) =
            encode::deserialize_partial(&data_bytes[cursor..])?;
        cursor += readed_len;
        let (parent_block_hash, readed_len): (BlockHash, usize) =
            encode::deserialize_partial(&data_bytes[cursor..])?;
        cursor += readed_len;
        let (merkle_branch, readed_len): (Vec<TxMerkleNode>, usize) =
            encode::deserialize_partial(&data_bytes[cursor..])?;
        cursor += readed_len;
        let (merkle_index, readed_len): (u32, usize) =
            encode::deserialize_partial(&data_bytes[cursor..])?;
        cursor += readed_len;
        let (chainmerkle_branch, readed_len): (Vec<TxMerkleNode>, usize) =
            encode::deserialize_partial(&data_bytes[cursor..])?;
        cursor += readed_len;
        let (chain_index, readed_len): (u32, usize) =
            encode::deserialize_partial(&data_bytes[cursor..])?;
        cursor += readed_len;
        let (parent_block, _readed_len): (BitcoinHeader, usize) =
            encode::deserialize_partial(&data_bytes[cursor..])?;
        let parent_block: Header = Header::from_block_header_vec(&serialize(&parent_block))?;

        let aux_data = AuxData {
            coinbase_tx: coinbase_tx.clone(),
            parent_block_hash,
            merkle_branch: merkle_branch.clone(),
            merkle_index,
            chainmerkle_branch: chainmerkle_branch.clone(),
            chain_index,
            parent_block,
        };

        let block1: Header = Header::from_block_header_vec(&serialize(&block1))?;
        Ok((block1, Some(aux_data)))
    }

    /// Get block header by bock height
    ///
    /// # Errors
    /// * issue with connection to the Bitcoin Node
    pub fn get_block_header_by_height(
        &self,
        height: u64,
    ) -> Result<Header, Box<dyn std::error::Error + Send + Sync>> {
        let block_hash = self.get_block_hash(height)?;
        Ok(self.get_aux_block_header(&block_hash)?.0)
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

#[cfg(not(feature = "zcash"))]
fn into_json<T>(val: T) -> Result<serde_json::Value, bitcoincore_rpc::Error>
where
    T: serde::ser::Serialize,
{
    Ok(serde_json::to_value(val)?)
}
