use crate::{abi::multicall3, config};

use alloy::{
    network::{Ethereum, TransactionBuilder},
    primitives::Bytes,
    providers::Provider,
    rpc::types::eth::{BlockId, TransactionRequest},
    sol_types::SolCall,
    transports::Transport,
};

use eyre::{eyre, Result, WrapErr};
use std::{iter, sync::Arc};
use tokio::task::JoinSet;
use tracing::instrument;

const MULTICALL_CHUNK_SIZE: usize = 1000;

#[instrument(skip_all, fields(calls_count = calls.len()))]
pub async fn multicall<T, P>(
    rpc_provider: Arc<P>,
    calls: Vec<multicall3::Call3>,
    block_id: Option<BlockId>,
) -> Result<Vec<multicall3::Result>>
where
    P: Provider<T, Ethereum> + 'static,
    T: Transport + Clone + 'static,
{
    let mut chunk_tasks = JoinSet::new();
    let chunk_iter = calls.chunks(MULTICALL_CHUNK_SIZE);
    let chunk_len = chunk_iter.len();

    chunk_iter.enumerate().for_each(|(idx, chunk)| {
        let tx = TransactionRequest::default()
            .with_to((*config::MULTICALL3_ADDRESS).into())
            .with_input(Bytes::from(
                multicall3::aggregate3Call {
                    calls: chunk.to_vec(),
                }
                .abi_encode(),
            ));

        let rpc_provider = Arc::clone(&rpc_provider);
        chunk_tasks.spawn(async move {
            rpc_provider
                .call(&tx, block_id)
                .await
                .wrap_err("multicall call failed")
                .and_then(|res| {
                    multicall3::aggregate3Call::abi_decode_returns(
                        res.as_ref(),
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to abi decode multicall")
                })
                .map(|res| (idx, res.returnData))
        });
    });

    let mut output = iter::repeat(Vec::new())
        .take(chunk_len)
        .collect::<Vec<Vec<multicall3::Result>>>();

    while let Some(chunk_res) = chunk_tasks.join_next().await {
        match chunk_res {
            Ok(Ok((chunk_idx, chunk_data))) => {
                output[chunk_idx] = chunk_data;
            }
            Ok(Err(e)) => {
                chunk_tasks.abort_all();
                return Err(eyre!("multicall failed due to execution error: {:?}", e));
            }
            Err(e) => {
                chunk_tasks.abort_all();
                return Err(eyre!("multicall failed due to join error: {:?}", e));
            }
        }
    }

    Ok(output.into_iter().flatten().collect())
}
