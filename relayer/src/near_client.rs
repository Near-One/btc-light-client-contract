use std::str::FromStr;

#[cfg(feature = "dogecoin")]
use bitcoin::consensus::serialize;
#[cfg(feature = "dogecoin")]
use bitcoincore_rpc::bitcoin::hashes::Hash;
use borsh::to_vec;
use btc_types::contract_args::InitArgs;
use btc_types::header::ExtendedHeader;
use log::info;
use merkle_tools::H256;
use near_crypto::InMemorySigner;
use near_jsonrpc_client::methods::broadcast_tx_async::RpcBroadcastTxAsyncResponse;
use near_jsonrpc_client::methods::tx::RpcTransactionResponse;
use near_jsonrpc_client::{methods, JsonRpcClient, MethodCallResult};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_jsonrpc_primitives::types::transactions::{RpcTransactionError, TransactionInfo};
use near_primitives::borsh;
use near_primitives::transaction::{Action, FunctionCallAction, SignedTransaction, Transaction};
use near_primitives::types::{AccountId, BlockReference};
use near_primitives::views::TxExecutionStatus;
use serde_json::{from_slice, json};

use crate::bitcoin_client::AuxData;
use tokio::time;

use crate::config::NearConfig;

const SUBMIT_BLOCKS: &str = "submit_blocks";
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

pub struct SignedSubmitTransaction {
    pub first_block_height: u64,
    pub last_block_height: u64,
    pub signed_tx: SignedTransaction,
}

#[cfg(feature = "dogecoin")]
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
            chain_id: aux_data.chain_index.try_into().unwrap(),
            parent_block: aux_data.parent_block,
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

    /// Initializes the BTC Light Client
    ///
    /// # Arguments
    /// * `args` - A reference to `InitArgs` containing the initialization parameters.
    ///
    /// # Returns
    /// A `Result` containing the `RpcTransactionResponse` if successful, or an error.
    ///
    /// # Errors
    /// This function can return an error in the following cases:
    /// - If submitting the transaction (`submit_tx`) fails, e.g., due to network issues or RPC failures.
    /// - If retrieving the transaction status (`get_tx_status`) fails, e.g., if the transaction is not found or the query times out.
    /// - If the contract execution itself fails on the NEAR side, resulting in a `FinalExecutionStatus::Failure`.
    ///   In this case, the function constructs and returns a formatted error with details from the contract failure.
    pub async fn init_contract(
        &self,
        args: &InitArgs,
    ) -> Result<RpcTransactionResponse, Box<dyn std::error::Error + Send + Sync>> {
        let tx_hash = self
            .submit_tx(
                self.sign_tx("init", json!({"args": args}).to_string().into_bytes(), 0)
                    .await?,
            )
            .await?;

        self.get_tx_status(tx_hash)
            .await
            .map_err(std::convert::Into::into)
            .map(|response| {
                if let Some(final_execution_outcome) = response.final_execution_outcome.clone() {
                    if let near_primitives::views::FinalExecutionStatus::Failure(err) =
                        final_execution_outcome.into_outcome().status
                    {
                        Err(format!("Transaction failed with error: {err:?}"))?;
                    }
                }
                Ok(response)
            })
            .and_then(|result| result)
    }

    /// Sign transaction of `submit_blocks` method call to the smart contract.
    ///
    /// # Arguments
    /// * `headers` - A vector of tuples containing block height, block header, and optional auxiliary data.
    /// * `batch_size` - The size of each batch of headers to be processed.
    ///
    /// # Returns
    /// A `Result` containing a vector of `SignedSubmitTransaction` if successful, or an error.
    ///
    /// # Errors
    /// This function can return an error in the following cases:
    /// * Serialization of the arguments fails (`to_vec`).
    /// * Signing of the transaction fails (`sign_tx`).
    /// * No blocks to submit in the current chunk.
    /// * No last block height in the current chunk.
    pub async fn sign_submit_blocks(
        &self,
        headers: Vec<(u64, btc_types::header::Header, Option<AuxData>)>,
        batch_size: usize,
    ) -> Result<Vec<SignedSubmitTransaction>, Box<dyn std::error::Error + Send + Sync>> {
        let mut signed_txs = Vec::new();

        for header_chunk in headers.chunks(batch_size) {
            for header in header_chunk {
                println!("Submit block {}", header.1.block_hash());
            }

            let Some(first_block_height) = header_chunk.first().map(|(height, _, _)| *height)
            else {
                return Err("No blocks to submit in the current chunk".into());
            };

            let Some(last_block_height) = header_chunk.last().map(|(height, _, _)| *height) else {
                return Err("No last block height in the current chunk".into());
            };

            #[cfg(feature = "dogecoin")]
            let args: Vec<_> = header_chunk
                .iter()
                .map(|(_, header, aux_data)| (header.clone(), get_aux_data(aux_data.clone())))
                .collect();

            #[cfg(not(feature = "dogecoin"))]
            let args: Vec<_> = header_chunk
                .iter()
                .map(|(_, header, _)| header.clone())
                .collect();

            signed_txs.push(SignedSubmitTransaction {
                first_block_height,
                last_block_height,
                signed_tx: self
                    .sign_tx(SUBMIT_BLOCKS, to_vec(&args)?, 5 * 10_u128.pow(23))
                    .await?,
            });
        }

        Ok(signed_txs)
    }

    /// Submitting block header to the smart contract.
    /// This method supports retries internally.
    ///
    /// # Errors
    /// * Transaction fails
    /// * Connection issue
    pub async fn submit_blocks(
        &self,
        signed_tx: SignedTransaction,
    ) -> Result<Result<RpcTransactionResponse, CustomError>, Box<dyn std::error::Error + Send + Sync>>
    {
        let sent_at = time::Instant::now();
        let tx_hash = self.submit_tx(signed_tx).await?;

        info!("Blocks submitted: tx_hash = {tx_hash:?}");

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
    ) -> Result<ExtendedHeader, Box<dyn std::error::Error + Send + Sync>> {
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
        block_hash: String,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
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
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
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
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let args = btc_types::contract_args::ProofArgs {
            tx_id: transaction_hash,
            tx_block_blockhash: transaction_block_blockhash,
            tx_index: transaction_position.try_into()?,
            merkle_proof,
            confirmations,
        };

        let tx_hash = self
            .submit_tx(
                self.sign_tx(VERIFY_TRANSACTION_INCLUSION, to_vec(&args)?, 0)
                    .await?,
            )
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

    /// Sign transaction for a method call to the smart contract.
    ///
    /// # Arguments
    /// * `method_name` - The name of the method to call on the smart contract.
    /// * `args` - A vector of bytes representing the arguments to pass to the method.
    /// * `deposit` - The amount of NEAR to deposit with the transaction.
    ///
    /// # Returns
    /// A `Result` containing a `SignedTransaction` if successful, or an error.
    ///
    /// # Errors
    /// * Access key query fails, e.g., due to network issues or RPC failures.
    /// * Current nonce cannot be extracted from the access key query response.
    /// * Signing the transaction fails, e.g., due to an invalid signer or public key.
    pub async fn sign_tx(
        &self,
        method_name: &str,
        args: Vec<u8>,
        deposit: u128,
    ) -> Result<SignedTransaction, Box<dyn std::error::Error + Send + Sync>> {
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
                gas: 300_000_000_000_000, // 300 TeraGas
                deposit,
            }))],
        };

        Ok(transaction.sign(&self.signer))
    }

    async fn submit_tx(
        &self,
        signed_tx: SignedTransaction,
    ) -> Result<RpcBroadcastTxAsyncResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self
            .client
            .call(methods::broadcast_tx_async::RpcBroadcastTxAsyncRequest {
                signed_transaction: signed_tx,
            })
            .await?)
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
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
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
