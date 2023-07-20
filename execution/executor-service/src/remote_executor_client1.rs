// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{error::Error, ExecuteBlockCommand, RemoteExecutionRequest, RemoteExecutionResult};
use aptos_logger::error;
use aptos_retrier::{fixed_retry_strategy, retry};
use aptos_secure_net::NetworkClient;
use aptos_state_view::StateView;
use aptos_types::{
    block_executor::partitioner::SubBlocksForShard,
    transaction::{analyzed_transaction::AnalyzedTransaction, TransactionOutput},
    vm_status::VMStatus,
};
use aptos_vm::sharded_block_executor::block_executor_client::BlockExecutorClient;
use std::{net::SocketAddr, sync::Mutex};

/// An implementation of [`BlockExecutorClient`] that supports executing blocks remotely.
pub struct RemoteExecutorClient1 {
    network_client: Mutex<NetworkClient>,
}

impl RemoteExecutorClient1 {
    pub fn new(server_address: SocketAddr, network_timeout_ms: u64) -> Self {
        let network_client = NetworkClient::new(
            "remote-executor-service".to_string(),
            server_address,
            network_timeout_ms,
        );
        Self {
            network_client: Mutex::new(network_client),
        }
    }

    fn execute_block_inner(
        &self,
        execution_request: RemoteExecutionRequest,
    ) -> Result<RemoteExecutionResult, Error> {
        let input_message = bcs::to_bytes(&execution_request)?;
        let mut network_client = self.network_client.lock().unwrap();
        network_client.write(&input_message)?;
        let bytes = network_client.read()?;
        Ok(bcs::from_bytes(&bytes)?)
    }

    fn execute_block_with_retry(
        &self,
        execution_request: RemoteExecutionRequest,
    ) -> RemoteExecutionResult {
        retry(fixed_retry_strategy(5, 20), || {
            let res = self.execute_block_inner(execution_request.clone());
            if let Err(e) = &res {
                error!("Failed to execute block: {:?}", e);
            }
            res
        })
        .unwrap()
    }
}

impl BlockExecutorClient for RemoteExecutorClient1 {
    fn execute_block<S: StateView + Sync>(
        &self,
        sub_blocks: SubBlocksForShard<AnalyzedTransaction>,
        state_view: &S,
        concurrency_level: usize,
        maybe_block_gas_limit: Option<u64>,
    ) -> Result<Vec<Vec<TransactionOutput>>, VMStatus> {
        let input = RemoteExecutionRequest::ExecuteBlock(ExecuteBlockCommand {
            sub_blocks,
            state_view: S::as_in_memory_state_view(state_view),
            concurrency_level,
            maybe_block_gas_limit,
        });

        self.execute_block_with_retry(input).inner
    }
}