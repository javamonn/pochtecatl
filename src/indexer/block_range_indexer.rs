use crate::{abi::IUniswapV2Pair, config, providers::RpcProvider, strategies::StrategyExecutor};

use super::{
    time_price_bar_store::TimePriceBarStore, Block, BlockBuilder, IndexedBlockMessage, Indexer,
    Resolution,
};

use alloy::{
    network::Ethereum,
    primitives::BlockNumber,
    providers::Provider,
    rpc::types::eth::{Filter, Log as RpcLog},
    sol_types::SolEvent,
    transports::Transport,
};

use eyre::{eyre, Result};
use std::{cmp::min, collections::BTreeMap, sync::Arc};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinSet,
};
use tracing::{debug, error, instrument};

pub struct BlockRangeIndexer<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    rpc_provider: Arc<RpcProvider<T, P>>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
    time_price_bar_store: Arc<TimePriceBarStore>,
}

impl<T, P> BlockRangeIndexer<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    pub fn new<B: Into<BlockNumber>>(
        rpc_provider: Arc<RpcProvider<T, P>>,
        start_block_number: B,
        end_block_number: B,
        is_backtest: bool,
    ) -> BlockRangeIndexer<T, P> {
        BlockRangeIndexer {
            rpc_provider,
            start_block_number: start_block_number.into(),
            end_block_number: end_block_number.into(),
            time_price_bar_store: Arc::new(TimePriceBarStore::new(
                Resolution::FiveMinutes,
                60,
                is_backtest,
            )),
        }
    }
}

const BLOCK_PARSER_CHUNK_SIZE: u64 = 50;
const BLOCK_PARSER_CONCURRENCY: u64 = 10;
const PARSED_BLOCK_CHANNEL_CAPACITY: usize = 1800;

#[instrument(skip(rpc_provider))]
async fn get_parsed_blocks<T, P>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
) -> Result<Vec<Block>>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    let filter = Filter::new()
        .from_block(start_block_number)
        .to_block(end_block_number)
        .event_signature(vec![
            IUniswapV2Pair::Sync::SIGNATURE_HASH,
            IUniswapV2Pair::Swap::SIGNATURE_HASH,
        ]);

    let (logs, start_block_header) = tokio::join!(
        rpc_provider.get_logs(&filter),
        rpc_provider
            .block_provider()
            .get_block_header(start_block_number)
    );

    let mut logs_by_block_number: BTreeMap<BlockNumber, Vec<RpcLog>> = logs
        .unwrap_or_else(|err| {
            error!(
                range_start_block_number = start_block_number,
                range_end_block_number = end_block_number,
                "get_logs failed: {:?}",
                err
            );
            Vec::new()
        })
        .into_iter()
        .fold(BTreeMap::new(), |mut acc, log| {
            acc.entry(log.block_number.unwrap())
                .or_insert_with(Vec::new)
                .push(log);

            acc
        });

    let start_block_timestamp = start_block_header.and_then(|header| {
        header
            .map(|h| h.timestamp.to::<u64>())
            .ok_or_else(|| eyre!("Expected block {} but found None", start_block_number))
    })?;

    // Parse the blocks
    let block_builders = (start_block_number..=end_block_number)
        .map(|block_number| {
            let block_logs = logs_by_block_number
                .remove(&block_number)
                .unwrap_or_default();

            // Estimate the block timestamp from the start block timestamp and average
            // block time. This avoids an rpc call for each block to lookup the header
            // or split the multicall.
            let estimated_block_timestamp = start_block_timestamp
                + (block_number - start_block_number) * *config::AVERAGE_BLOCK_TIME_SECONDS;

            BlockBuilder::new(block_number, estimated_block_timestamp, &block_logs)
        })
        .collect();

    BlockBuilder::build_many(block_builders, Arc::clone(&rpc_provider)).await
}

#[instrument(skip(rpc_provider, parsed_block_sender))]
async fn block_parser_task<T, P>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
    parsed_block_sender: Sender<Block>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    for range_start_block_number in (start_block_number..=end_block_number)
        .step_by((BLOCK_PARSER_CHUNK_SIZE * BLOCK_PARSER_CONCURRENCY) as usize)
    {
        let range_end_block_number = min(
            range_start_block_number + (BLOCK_PARSER_CHUNK_SIZE * BLOCK_PARSER_CONCURRENCY),
            end_block_number,
        );

        let mut subtasks = JoinSet::new();
        for (subtask_idx, subtask_start_block_number) in (range_start_block_number
            ..=range_end_block_number)
            .step_by(BLOCK_PARSER_CHUNK_SIZE as usize)
            .enumerate()
        {
            let subtask_end_block_number = min(
                subtask_start_block_number + BLOCK_PARSER_CHUNK_SIZE,
                range_end_block_number,
            );

            let rpc_provider = Arc::clone(&rpc_provider);
            subtasks.spawn(async move {
                get_parsed_blocks(
                    rpc_provider,
                    subtask_start_block_number,
                    subtask_end_block_number,
                )
                .await
                .map(|blocks| (subtask_idx, blocks))
            });
        }

        let mut parsed_blocks: Vec<Option<Block>> =
            Vec::with_capacity((range_end_block_number - range_start_block_number) as usize);
        (range_start_block_number..=range_end_block_number).for_each(|_| {
            parsed_blocks.push(None);
        });

        while let Some(subtask) = subtasks.join_next().await {
            match subtask {
                Ok(Ok((subtask_idx, blocks))) => {
                    let subtask_start_block_number =
                        range_start_block_number + subtask_idx as u64 * BLOCK_PARSER_CHUNK_SIZE;
                    blocks.into_iter().enumerate().for_each(|(idx, block)| {
                        let block_number = subtask_start_block_number + idx as u64;
                        parsed_blocks[(block_number - range_start_block_number) as usize] =
                            Some(block);
                    });
                }
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(eyre!("block_parser_task failed due to join error: {:?}", e)),
            }
        }

        debug!(
            remaining_channel_capacity = {
                let p = parsed_block_sender.capacity() as f64
                    / parsed_block_sender.max_capacity() as f64;

                format!("{:.2}%", p * 100.0)
            },
            "dispatching parsed blocks"
        );

        for parsed_block in parsed_blocks.into_iter() {
            match parsed_block {
                Some(parsed_block) => {
                    parsed_block_sender.send(parsed_block).await?;
                }
                None => {
                    return Err(eyre!("Missing block in parsed_blocks"));
                }
            }
        }
    }

    Ok(())
}

#[instrument(skip_all)]
async fn strategy_executor_task<T, P, S>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    mut parsed_block_receiver: Receiver<Block>,
    time_price_bar_store: Arc<TimePriceBarStore>,
    strategy_executor: S,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
    S: StrategyExecutor,
{
    while let Some(parsed_block) = parsed_block_receiver.recv().await {
        time_price_bar_store
            .insert_block(Arc::clone(&rpc_provider), &parsed_block)
            .await?;

        let indexed_block_message =
            IndexedBlockMessage::from_block(&parsed_block);

        strategy_executor
            .on_indexed_block_message(indexed_block_message, &time_price_bar_store)
            .await?;
    }

    Ok(())
}

impl<T, P> Indexer for BlockRangeIndexer<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    async fn exec<S>(&mut self, strategy_executor: S) -> Result<()>
    where
        S: StrategyExecutor + Send + 'static,
    {
        // The channel used driving the overall backtest process
        let (parsed_block_sender, parsed_block_receiver) = channel(PARSED_BLOCK_CHANNEL_CAPACITY);

        let strategy_executor_join_handle = {
            let rpc_provider = Arc::clone(&self.rpc_provider);
            let time_price_bar_store = Arc::clone(&self.time_price_bar_store);

            tokio::spawn(async move {
                strategy_executor_task(
                    rpc_provider,
                    parsed_block_receiver,
                    time_price_bar_store,
                    strategy_executor,
                )
                .await
            })
        };
        let parser_join_handle = {
            let rpc_provider = Arc::clone(&self.rpc_provider);
            let start_block_number = self.start_block_number;
            let end_block_number = self.end_block_number;

            tokio::spawn(async move {
                block_parser_task(
                    rpc_provider,
                    start_block_number,
                    end_block_number,
                    parsed_block_sender,
                )
                .await
            })
        };

        let (strategy_executor_result, parser_result) =
            tokio::join!(strategy_executor_join_handle, parser_join_handle);

        match (strategy_executor_result, parser_result) {
            (Ok(Ok(())), Ok(Ok(()))) => Ok(()),
            (Ok(Err(e)), _) => Err(eyre!("failed due to strategy_executor error: {:?}", e)),
            (_, Ok(Err(e))) => Err(eyre!("failed due to parser error: {:?}", e)),
            (Err(e), _) => Err(eyre!("failed due to strategy_executor join error: {:?}", e)),
            (_, Err(e)) => Err(eyre!("failed due to parser join error: {:?}", e)),
        }
    }

    fn time_price_bar_store(&self) -> Arc<TimePriceBarStore> {
        Arc::clone(&self.time_price_bar_store)
    }
}

#[cfg(test)]
mod tests {
    use super::get_parsed_blocks;

    use crate::{config, providers::rpc_provider::new_http_signer_provider};

    use eyre::Result;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_get_parsed_blocks() -> Result<()> {
        let rpc_provider = Arc::new(new_http_signer_provider(&config::RPC_URL, None).await?);

        let start_block_number = 13868901;
        let end_block_number = 13868921;

        let (parsed_blocks, end_block_header) = tokio::join!(
            get_parsed_blocks(
                Arc::clone(&rpc_provider),
                start_block_number,
                end_block_number
            ),
            rpc_provider
                .block_provider()
                .get_block_header(end_block_number)
        );

        let parsed_blocks = parsed_blocks?;
        let end_block_header = end_block_header?;

        assert_eq!(parsed_blocks.len(), 21);
        assert!(parsed_blocks
            .first()
            .is_some_and(|b| b.block_number == 13868901 && b.uniswap_v2_pairs.len() == 11));
        assert!(parsed_blocks
            .last()
            .is_some_and(|b| b.block_number == 13868921
                && b.block_timestamp == end_block_header.unwrap().timestamp.to::<u64>()));

        Ok(())
    }
}
