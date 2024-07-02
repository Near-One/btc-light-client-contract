use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_jsonrpc_primitives::types::transactions::{RpcTransactionError, TransactionInfo};
use near_primitives::transaction::{Action, FunctionCallAction, Transaction};
use near_primitives::types::{AccountId, BlockReference};
use near_primitives::views::TxExecutionStatus;

use bitcoincore_rpc::bitcoin::block::Header;
use serde_json::{from_slice, json};
use std::str::FromStr;

use tokio::time;

use crate::config::Config;

const SUBMIT_BLOCK_HEADER: &str = "submit_block_header";
const GET_BLOCK_HEADER: &str = "get_block_header";
const VERIFY_TRANSACTION_INCLUSION: &str = "verify_transaction_inclusion";
const RECEIVE_LAST_N_BLOCKS: &str = "receive_last_n_blocks";

#[derive(Debug, Clone)]
pub struct Client {
    config: Config,
}

impl Client {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Submitting block header to the smart contract.
    /// This method supports retries internally.
    pub async fn submit_block_header(
        &self,
        header: Header,
    ) -> Result<Result<(), usize>, Box<dyn std::error::Error>> {
        let client = JsonRpcClient::connect(&self.config.near.endpoint);
        let signer_account_id = AccountId::from_str(&self.config.near.account_name).unwrap();
        let signer_secret_key =
            near_crypto::SecretKey::from_str(&self.config.near.secret_key).unwrap();
        let args = json!({
            "block_header": serde_json::to_value(header).expect("bitcoin should be validate before")
        });

        let signer = near_crypto::InMemorySigner::from_secret_key(
            signer_account_id.clone(),
            signer_secret_key,
        );

        let access_key_query_response = client
            .call(methods::query::RpcQueryRequest {
                block_reference: BlockReference::latest(),
                request: near_primitives::views::QueryRequest::ViewAccessKey {
                    account_id: signer.account_id.clone(),
                    public_key: signer.public_key.clone(),
                },
            })
            .await?;

        let current_nonce = match access_key_query_response.kind {
            QueryResponseKind::AccessKey(access_key) => access_key.nonce,
            _ => Err("failed to extract current nonce")?,
        };

        let transaction = Transaction {
            signer_id: signer.account_id.clone(),
            public_key: signer.public_key.clone(),
            nonce: current_nonce + 1,
            receiver_id: signer_account_id,
            block_hash: access_key_query_response.block_hash,
            actions: vec![Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: SUBMIT_BLOCK_HEADER.to_string(),
                args: args.to_string().into_bytes(),
                gas: 100_000_000_000_000, // 100 TeraGas
                deposit: 0,
            }))],
        };

        let request = methods::broadcast_tx_async::RpcBroadcastTxAsyncRequest {
            signed_transaction: transaction.sign(&signer),
        };

        let sent_at = time::Instant::now();
        let tx_hash = client.call(request).await?;

        loop {
            let response = client
                .call(methods::tx::RpcTransactionStatusRequest {
                    transaction_info: TransactionInfo::TransactionId {
                        tx_hash,
                        sender_account_id: signer.account_id.clone(),
                    },
                    wait_until: TxExecutionStatus::Executed,
                })
                .await;
            let received_at = time::Instant::now();
            let delta = (received_at - sent_at).as_secs();

            if delta > 120 {
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
                    println!("response gotten after: {delta}s");
                    println!("response: {response:#?}");
                    return Ok(Ok(()));
                }
            }
        }
    }

    #[allow(dead_code)]
    pub async fn read_last_block_header(&self) -> Result<Header, Box<dyn std::error::Error>> {
        let node_url = self.config.near.endpoint.clone();
        let contract_id = self.config.near.account_name.clone();

        let args = json!({});
        let client = near_jsonrpc_client::JsonRpcClient::connect(node_url);

        let read_request = near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::Finality(
                near_primitives::types::Finality::Final,
            ),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: contract_id.parse().unwrap(),
                method_name: GET_BLOCK_HEADER.to_string(),
                args: args.to_string().into_bytes().into(),
            },
        };
        let response = client.call(read_request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            let header = from_slice::<Header>(&result.result)?;
            println!("{header:#?}");
            println!("Block Height: {}", response.block_height);
            println!("Block Hash: {}", response.block_hash);

            Ok(header)
        } else {
            Err("failed to read block header")?
        }
    }

    pub async fn receive_last_n_blocks(
        &self,
        n: usize,
        shift_from_the_end: usize,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let node_url = self.config.near.endpoint.clone();
        let contract_id = self.config.near.account_name.clone();

        let args = json!({
            "n": n,
            "shift_from_the_end": shift_from_the_end,
        });
        let client = near_jsonrpc_client::JsonRpcClient::connect(node_url);

        let read_request = near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::Finality(
                near_primitives::types::Finality::Final,
            ),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: contract_id.parse().unwrap(),
                method_name: RECEIVE_LAST_N_BLOCKS.to_string(),
                args: args.to_string().into_bytes().into(),
            },
        };
        let response = client.call(read_request).await?;

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
        transaction_hash: String,
        transaction_position: usize,
        transaction_block_height: usize,
        merkle_proof: Vec<String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let node_url = self.config.near.endpoint.clone();
        let contract_id = self.config.near.account_name.clone();

        let args = serde_json::json!({
            "txid": transaction_hash,
            "tx_block_height": transaction_block_height,
            "tx_index": transaction_position,
            "merkle_proof": merkle_proof,
            "confirmations": 0,
        });

        let client = near_jsonrpc_client::JsonRpcClient::connect(node_url);

        let read_request = near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::Finality(
                near_primitives::types::Finality::Final,
            ),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: contract_id.parse().unwrap(),
                method_name: VERIFY_TRANSACTION_INCLUSION.to_string(),
                args: args.to_string().into_bytes().into(),
            },
        };
        let response = client.call(read_request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            let included = from_slice::<bool>(&result.result)?;
            println!("{included:#?}");
            Ok(included)
        } else {
            Err("failed to read block header")?
        }
    }
}
