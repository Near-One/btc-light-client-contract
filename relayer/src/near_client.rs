use btc_types::header::ExtendedHeader;
use merkle_tools::H256;
use near_jsonrpc_client::methods::tx::RpcTransactionResponse;
use near_jsonrpc_client::{methods, JsonRpcClient, MethodCallResult};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_jsonrpc_primitives::types::transactions::{RpcTransactionError, TransactionInfo};
use near_primitives::transaction::{Action, FunctionCallAction, Transaction};
use near_primitives::types::{AccountId, BlockReference};
use near_primitives::views::TxExecutionStatus;

use bitcoin::consensus::serialize;
use bitcoin::BlockHash;
use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::hashes::Hash;
use borsh::to_vec;
use log::info;
use near_crypto::InMemorySigner;
use near_jsonrpc_client::methods::broadcast_tx_async::RpcBroadcastTxAsyncResponse;
use near_primitives::borsh;
use serde::Serialize;
use serde_json::{from_slice, json};
use std::str::FromStr;

use crate::bitcoin_client::AuxData;
use tokio::time;

use crate::config::NearConfig;

const SUBMIT_BLOCKS: &str = "submit_blocks_aux";
const GET_LAST_BLOCK_HEADER: &str = "get_last_block_header";
#[allow(dead_code)]
const VERIFY_TRANSACTION_INCLUSION: &str = "verify_transaction_inclusion";
const RECEIVE_LAST_N_BLOCKS: &str = "get_last_n_blocks_hashes";
const GET_HEIGHT_BY_BLOCK_HASH: &str = "get_height_by_block_hash";

#[derive(thiserror::Error, Debug)]
pub enum CustomError {
    #[error("Prev Block Not Found")]
    PrevBlockNotFound,
    #[error("Tx execution Error: {0:?}")]
    TxExecutionError(String),
}

#[derive(Clone)]
pub struct NearClient {
    client: JsonRpcClient,
    signer: InMemorySigner,
    btc_light_client_account_id: AccountId,
    transaction_timeout_sec: u64,
}

fn get_btc_header(header: Header) -> btc_types::header::Header {
    btc_types::header::Header {
        version: header.version.to_consensus(),
        prev_block_hash: header.prev_blockhash.to_byte_array().into(),
        merkle_root: header.merkle_root.to_byte_array().into(),
        time: header.time,
        bits: header.bits.to_consensus(),
        nonce: header.nonce,
    }
}

fn get_aux_data(aux_data: Option<AuxData>) -> Option<btc_types::aux::AuxData> {
    match aux_data {
        None => None,
        Some(aux_data) => Some(btc_types::aux::AuxData {
            coinbase_tx: serialize(&aux_data.coinbase_tx),
            merkle_proof: aux_data
                .merkle_branch
                .iter()
                .map(|h| H256::from(h.to_raw_hash().to_byte_array()))
                .collect(),
            chain_merkle_proof: aux_data
                .chainmerkle_branch
                .iter()
                .map(|h| H256::from(h.to_raw_hash().to_byte_array()))
                .collect(),
            chain_id: aux_data.chain_index as usize,
            parent_block: get_btc_header(aux_data.parent_block),
        }),
    }
}

impl NearClient {
    /// Create new Near client
    ///
    /// # Panics
    /// * incorrect near endpoint
    /// * incorrect `private_key` or `account_id`
    /// * incorrect `btc_light_client_account_id`
    #[must_use]
    pub fn new(config: &NearConfig) -> Self {
        let client = JsonRpcClient::connect(&config.endpoint);

        let (signer_account_id, signer_secret_key) =
            if let Some(near_credentials_path) = config.near_credentials_path.clone() {
                let data = std::fs::read_to_string(near_credentials_path).unwrap();
                let res: serde_json::Value = serde_json::from_str(&data).unwrap();

                let private_key = res["private_key"].to_string().replace('\"', "");
                let private_key = near_crypto::SecretKey::from_str(private_key.as_str()).unwrap();

                let account_id = res["account_id"].to_string().replace('\"', "");
                let account_id = AccountId::from_str(account_id.as_str()).unwrap();
                (account_id, private_key)
            } else {
                (
                    AccountId::from_str(&config.account_name.clone().unwrap()).unwrap(),
                    near_crypto::SecretKey::from_str(&config.secret_key.clone().unwrap()).unwrap(),
                )
            };

        let signer = near_crypto::InMemorySigner::from_secret_key(
            signer_account_id.clone(),
            signer_secret_key,
        );

        Self {
            client,
            signer,
            btc_light_client_account_id: config
                .btc_light_client_account_id
                .clone()
                .parse()
                .unwrap(),
            transaction_timeout_sec: config.transaction_timeout_sec,
        }
    }

    /// Submitting block header to the smart contract.
    /// This method supports retries internally.
    ///
    /// # Errors
    /// * Transaction fails
    /// * Connection issue
    pub async fn submit_blocks(
        &self,
        headers: Vec<(Header, Option<AuxData>)>,
    ) -> Result<Result<RpcTransactionResponse, CustomError>, Box<dyn std::error::Error>> {
        let args: Vec<_> = headers
            .iter()
            .map(|header| {
                (
                    get_btc_header((*header).0),
                    get_aux_data((*header).1.clone()),
                )
            })
            .collect();

        let sent_at = time::Instant::now();
        let tx_hash = self.submit_tx(SUBMIT_BLOCKS, to_vec(&args)?).await?;
        info!("Blocks submitted: tx_hash = {:?}", tx_hash);

        loop {
            let response = self.get_tx_status(tx_hash).await;
            let received_at = time::Instant::now();
            let delta = (received_at - sent_at).as_secs();

            if delta > self.transaction_timeout_sec {
                Err("time limit exceeded for the transaction to be recognized")?;
            }

            match response {
                Err(err) => match err.handler_error() {
                    Some(
                        RpcTransactionError::TimeoutError
                        | RpcTransactionError::UnknownTransaction { .. },
                    ) => {
                        time::sleep(time::Duration::from_secs(2)).await;
                        continue;
                    }
                    _ => Err(err)?,
                },
                Ok(response) => {
                    println!("Success response gotten after: {delta}s");
                    return Ok(Self::parse_submit_blocks_response(response));
                }
            }
        }
    }

    fn parse_submit_blocks_response(
        response: RpcTransactionResponse,
    ) -> Result<RpcTransactionResponse, CustomError> {
        if let Some(final_execution_outcome) = response.final_execution_outcome.clone() {
            if let near_primitives::views::FinalExecutionStatus::Failure(err) =
                final_execution_outcome.into_outcome().status
            {
                if format!("{err:?}").contains("PrevBlockNotFound") {
                    Err(CustomError::PrevBlockNotFound)?;
                } else {
                    Err(CustomError::TxExecutionError(format!("{err:?}")))?;
                }
            }
        }

        Ok(response)
    }

    /// Get last Bitcoin Block Header on Near
    ///
    /// # Errors
    /// * Connection issue
    pub async fn get_last_block_header(
        &self,
    ) -> Result<ExtendedHeader, Box<dyn std::error::Error>> {
        let args = json!({});
        let result = self
            .submit_view_tx(GET_LAST_BLOCK_HEADER, args.to_string().into_bytes())
            .await?;

        let header = from_slice::<ExtendedHeader>(&result)?;
        println!("Block Height: {}", header.block_height);
        println!("Block Hash: {}", header.block_hash);

        Ok(header)
    }

    /// Check that block already submitted to Near
    /// If the block is on Near but from fork
    /// the function return false
    ///
    /// # Errors
    /// * Connection issue
    pub async fn is_block_hash_exists(
        &self,
        block_hash: BlockHash,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let args = json!({
            "blockhash": block_hash,
        });

        let result = self
            .submit_view_tx(GET_HEIGHT_BY_BLOCK_HASH, args.to_string().into_bytes())
            .await?;

        let block_height = from_slice::<Option<u64>>(&result)?;
        Ok(block_height.is_some())
    }

    /// Get last n Bitcoin block hashes from Near
    ///
    /// # Errors
    /// * Connection issue
    pub async fn get_last_n_blocks_hashes(
        &self,
        n: u64,
        shift_from_the_end: u64,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let args = json!({
            "skip": shift_from_the_end,
            "limit": n,
        });

        let result = self
            .submit_view_tx(RECEIVE_LAST_N_BLOCKS, args.to_string().into_bytes())
            .await?;

        let block_hashes = from_slice::<Vec<String>>(&result)?;
        println!("{block_hashes:#?}");
        Ok(block_hashes)
    }

    /// Verify transaction inclusion
    ///
    /// # Errors
    /// * Connection issue
    /// * Transaction fails
    #[allow(dead_code)]
    pub async fn verify_transaction_inclusion(
        &self,
        transaction_hash: H256,
        transaction_position: usize,
        transaction_block_blockhash: H256,
        merkle_proof: Vec<H256>,
        confirmations: u64,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let args = btc_types::contract_args::ProofArgs {
            tx_id: transaction_hash,
            tx_block_blockhash: transaction_block_blockhash,
            tx_index: transaction_position.try_into()?,
            merkle_proof,
            confirmations,
        };

        let tx_hash = self
            .submit_tx(VERIFY_TRANSACTION_INCLUSION, to_vec(&args)?)
            .await?;
        let response = self.get_tx_status(tx_hash).await?;

        match response
            .final_execution_outcome
            .clone()
            .ok_or("No final execution outcome")?
            .into_outcome()
            .status
        {
            near_primitives::views::FinalExecutionStatus::SuccessValue(value) => {
                let parsed_output = String::from_utf8(value.clone())?;
                println!(
                    "Transaction succeeded with result: {:?}",
                    String::from_utf8(value.clone())
                );

                if parsed_output == "true" {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            near_primitives::views::FinalExecutionStatus::Failure(err) => {
                Err(format!("Transaction failed with error: {err:?}"))?
            }
            _ => Err(format!(
                "Transaction status: {:?}",
                response
                    .final_execution_outcome
                    .ok_or("No final execution outcome")?
                    .into_outcome()
                    .status
            ))?,
        }
    }

    async fn submit_tx(
        &self,
        method_name: &str,
        args: Vec<u8>,
    ) -> Result<RpcBroadcastTxAsyncResponse, Box<dyn std::error::Error>> {
        let access_key_query_response = self
            .client
            .call(methods::query::RpcQueryRequest {
                block_reference: BlockReference::latest(),
                request: near_primitives::views::QueryRequest::ViewAccessKey {
                    account_id: self.signer.account_id.clone(),
                    public_key: self.signer.public_key.clone(),
                },
            })
            .await?;

        let current_nonce = match access_key_query_response.kind {
            QueryResponseKind::AccessKey(access_key) => access_key.nonce,
            _ => Err("failed to extract current nonce")?,
        };

        let transaction = Transaction {
            signer_id: self.signer.account_id.clone(),
            public_key: self.signer.public_key.clone(),
            nonce: current_nonce + 1,
            receiver_id: self.btc_light_client_account_id.clone(),
            block_hash: access_key_query_response.block_hash,
            actions: vec![Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: method_name.to_string(),
                args,
                gas: 300_000_000_000_000,     // 300 TeraGas
                deposit: 5 * 10_u128.pow(23), // 0.5 Near
            }))],
        };

        let request = methods::broadcast_tx_async::RpcBroadcastTxAsyncRequest {
            signed_transaction: transaction.sign(&self.signer),
        };

        Ok(self.client.call(request).await?)
    }

    async fn get_tx_status(
        &self,
        tx_hash: RpcBroadcastTxAsyncResponse,
    ) -> MethodCallResult<RpcTransactionResponse, RpcTransactionError> {
        self.client
            .call(methods::tx::RpcTransactionStatusRequest {
                transaction_info: TransactionInfo::TransactionId {
                    tx_hash,
                    sender_account_id: self.signer.account_id.clone(),
                },
                wait_until: TxExecutionStatus::Executed,
            })
            .await
    }

    async fn submit_view_tx(
        &self,
        method_name: &str,
        args: Vec<u8>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let read_request = near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::Finality(
                near_primitives::types::Finality::Final,
            ),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: self.btc_light_client_account_id.clone(),
                method_name: method_name.to_string(),
                args: args.into(),
            },
        };
        let response = self.client.call(read_request).await?;
        if let QueryResponseKind::CallResult(result) = response.kind {
            Ok(result.result)
        } else {
            Err("the view tx fail")?
        }
    }
}
