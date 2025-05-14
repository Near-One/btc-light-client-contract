use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::hashes::Hash;
use bitcoincore_rpc::bitcoin::BlockHash;
use bitcoincore_rpc::jsonrpc::minreq_http::HttpError;
use bitcoincore_rpc::jsonrpc::Transport;
use bitcoincore_rpc::{jsonrpc, RpcApi};
use jsonrpc::{Request, Response};

use crate::config::Config;

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

    pub fn basic_auth(user: String, pass: Option<String>) -> String {
        let mut s = user;
        s.push(':');
        if let Some(pass) = pass {
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
        let client = CustomMinreqHttpTransport {
            url: config.bitcoin.endpoint.clone(),
            timeout: std::time::Duration::from_secs(15),
            basic_auth: Some(CustomMinreqHttpTransport::basic_auth(
                config.bitcoin.node_user.clone(),
                Some(config.bitcoin.node_password.clone()),
            )),
            headers: config.bitcoin.node_headers.clone().unwrap_or_default(),
        };
        println!("client: {:?}", client.headers);
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
