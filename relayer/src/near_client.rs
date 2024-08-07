use btc_types::header::ExtendedHeader;
use merkle_tools::H256;
use near_jsonrpc_client::methods::tx::RpcTransactionResponse;
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_jsonrpc_primitives::types::transactions::{RpcTransactionError, TransactionInfo};
use near_primitives::transaction::{Action, FunctionCallAction, Transaction};
use near_primitives::types::{AccountId, BlockReference};
use near_primitives::views::TxExecutionStatus;

use bitcoincore_rpc::bitcoin::block::Header;
use bitcoincore_rpc::bitcoin::hashes::Hash;
use borsh::to_vec;
use near_crypto::InMemorySigner;
use near_primitives::borsh;
use serde_json::{from_slice, json};
use std::str::FromStr;
use bitcoin::BlockHash;
use tokio::time;

use crate::config::NearConfig;

const SUBMIT_BLOCKS: &str = "submit_blocks";
const GET_LAST_BLOCK_HEADER: &str = "get_last_block_header";
const VERIFY_TRANSACTION_INCLUSION: &str = "verify_transaction_inclusion";
const RECEIVE_LAST_N_BLOCKS: &str = "get_last_n_blocks_hashes";
const GET_HEIGHT_BY_BLOCK_HASH: &str = "get_height_by_block_hash";

#[derive(thiserror::Error, Debug)]
pub enum CustomError {
    #[error("Prev Block Not Found")]
    PrevBlockNotFound,
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

impl NearClient {
    pub fn new(config: &NearConfig) -> Self {
        let client = JsonRpcClient::connect(&config.endpoint);

        let signer_account_id = AccountId::from_str(&config.account_name).unwrap();
        let signer_secret_key = near_crypto::SecretKey::from_str(&config.secret_key).unwrap();
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
    pub async fn submit_blocks(
        &self,
        headers: Vec<Header>,
    ) -> Result<Result<RpcTransactionResponse, CustomError>, Box<dyn std::error::Error>> {
        let args: Vec<_> = headers
            .iter()
            .map(|header| {
                println!("Submit block {}", header.block_hash());
                get_btc_header(*header)
            })
            .collect();

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
                method_name: SUBMIT_BLOCKS.to_string(),
                args: to_vec(&args).expect("error on headers serialisation"),
                gas: 100_000_000_000_000, // 100 TeraGas
                deposit: 0,
            }))],
        };

        let request = methods::broadcast_tx_async::RpcBroadcastTxAsyncRequest {
            signed_transaction: transaction.sign(&self.signer),
        };

        let sent_at = time::Instant::now();
        let tx_hash = self.client.call(request).await?;

        loop {
            let response = self
                .client
                .call(methods::tx::RpcTransactionStatusRequest {
                    transaction_info: TransactionInfo::TransactionId {
                        tx_hash,
                        sender_account_id: self.signer.account_id.clone(),
                    },
                    wait_until: TxExecutionStatus::Executed,
                })
                .await;
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
                }
            }
        }

        Ok(response)
    }

    pub async fn get_last_block_header(
        &self,
    ) -> Result<ExtendedHeader, Box<dyn std::error::Error>> {
        let args = json!({});

        let read_request = near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::Finality(
                near_primitives::types::Finality::Final,
            ),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: self.btc_light_client_account_id.clone(),
                method_name: GET_LAST_BLOCK_HEADER.to_string(),
                args: args.to_string().into_bytes().into(),
            },
        };
        let response = self.client.call(read_request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            let header = from_slice::<ExtendedHeader>(&result.result)?;
            println!("Block Height: {}", response.block_height);
            println!("Block Hash: {}", response.block_hash);

            Ok(header)
        } else {
            Err("failed to read block header")?
        }
    }

    pub async fn is_block_hash_exists(&self, block_hash: BlockHash) -> Result<bool, Box<dyn std::error::Error>> {
        let args = json!({
            "blockhash": block_hash,
        });

        let read_request = methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(
                near_primitives::types::Finality::Final,
            ),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: self.btc_light_client_account_id.clone(),
                method_name: GET_HEIGHT_BY_BLOCK_HASH.to_string(),
                args: args.to_string().into_bytes().into(),
            },
        };
        let response = self.client.call(read_request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            let block_height = from_slice::<Option<u64>>(&result.result)?;
            Ok(block_height.is_some())
        } else {
            Err("failed to get block height by hash")?
        }
    }

    pub async fn get_last_n_blocks_hashes(
        &self,
        n: u64,
        shift_from_the_end: u64,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let args = json!({
            "skip": shift_from_the_end,
            "limit": n,
        });

        let read_request = near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::Finality(
                near_primitives::types::Finality::Final,
            ),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: self.btc_light_client_account_id.clone(),
                method_name: RECEIVE_LAST_N_BLOCKS.to_string(),
                args: args.to_string().into_bytes().into(),
            },
        };
        let response = self.client.call(read_request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            let block_hashes = from_slice::<Vec<String>>(&result.result)?;
            println!("{block_hashes:#?}");
            Ok(block_hashes)
        } else {
            Err("failed to read block header")?
        }
    }

    pub async fn verify_transaction_inclusion(
        &self,
        transaction_hash: H256,
        transaction_position: usize,
        transaction_block_blockhash: H256,
        merkle_proof: Vec<H256>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
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

        let args = btc_types::contract_args::ProofArgs {
            tx_id: transaction_hash,
            tx_block_blockhash: transaction_block_blockhash,
            tx_index: transaction_position.try_into().unwrap(),
            merkle_proof,
            confirmations: 0,
        };

        let transaction = Transaction {
            signer_id: self.signer.account_id.clone(),
            public_key: self.signer.public_key.clone(),
            nonce: current_nonce + 1,
            receiver_id: self.btc_light_client_account_id.clone(),
            block_hash: access_key_query_response.block_hash,
            actions: vec![Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: VERIFY_TRANSACTION_INCLUSION.to_string(),
                args: to_vec(&args).expect("error on ProofArgs serialisation"),
                gas: 100_000_000_000_000, // 100 TeraGas
                deposit: 0,
            }))],
        };

        let request = methods::broadcast_tx_async::RpcBroadcastTxAsyncRequest {
            signed_transaction: transaction.sign(&self.signer),
        };

        let tx_hash = self.client.call(request).await?;

        let response = self
            .client
            .call(methods::tx::RpcTransactionStatusRequest {
                transaction_info: TransactionInfo::TransactionId {
                    tx_hash,
                    sender_account_id: self.signer.account_id.clone(),
                },
                wait_until: TxExecutionStatus::Executed,
            })
            .await?;

        match response
            .final_execution_outcome
            .clone()
            .unwrap()
            .into_outcome()
            .status
        {
            near_primitives::views::FinalExecutionStatus::SuccessValue(value) => {
                let parsed_output = String::from_utf8(value.clone()).unwrap();
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
                    .unwrap()
                    .into_outcome()
                    .status
            ))?,
        }
    }
}
