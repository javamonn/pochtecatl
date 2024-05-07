use crate::{abi::IUniswapV2Pair, providers::RpcProvider};

use super::{
    time_price_bar_store::TimePriceBarStore, Block, IndexedBlockMessage, Indexer, Resolution,
};

use alloy::{
    network::Ethereum,
    primitives::BlockNumber,
    providers::Provider,
    rpc::types::eth::{Filter, Log as RpcLog},
    sol_types::SolEvent,
    transports::Transport,
};

use eyre::{Context, Result};
use std::{cmp::min, collections::BTreeMap, sync::Arc};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::{JoinHandle, JoinSet},
};
use tracing::{debug, error, instrument, warn};

pub struct BlockRangeIndexer {
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
    index_handle: Option<JoinHandle<Result<()>>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
}

impl BlockRangeIndexer {
    pub fn new<B: Into<BlockNumber>>(
        start_block_number: B,
        end_block_number: B,
        is_backtest: bool,
    ) -> BlockRangeIndexer {
        BlockRangeIndexer {
            start_block_number: start_block_number.into(),
            end_block_number: end_block_number.into(),
            index_handle: None,
            time_price_bar_store: Arc::new(TimePriceBarStore::new(
                Resolution::FiveMinutes,
                60,
                is_backtest,
            )),
        }
    }
}

const BLOCK_RANGE_STEP_BY: u64 = 100;

#[instrument(skip(rpc_provider))]
async fn get_parsed_blocks<T, P>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
) -> Result<BTreeMap<BlockNumber, Block>>
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

    let (logs, mut headers_by_block_number) = tokio::join!(
        rpc_provider.get_logs(&filter),
        rpc_provider
            .block_provider()
            .get_block_headers_by_range(start_block_number, end_block_number)
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

    // Parse the blocks
    let mut tasks = JoinSet::new();
    let mut output = BTreeMap::new();
    for block_number in start_block_number..=end_block_number {
        match headers_by_block_number.remove(&block_number).flatten() {
            Some(block_header) => {
                let rpc_provider = Arc::clone(&rpc_provider);
                let block_logs = logs_by_block_number
                    .remove(&block_number)
                    .unwrap_or_default();

                tasks.spawn(async move {
                    Block::parse(rpc_provider, &block_header, &block_logs)
                        .await
                        .wrap_err_with(|| format!("Block::parse failed for block {}", block_number))
                });
            }
            None => {
                warn!(
                    block_number = block_number,
                    "Expected header for block but found none"
                );
            }
        }
    }

    while let Some(block) = tasks.join_next().await {
        match block {
            Ok(Ok(block)) => {
                output.insert(block.block_number, block);
            }
            Ok(Err(err)) => {
                error!("Block::parse error: {:?}", err);
            }
            Err(err) => {
                error!("join_next error: {:?}", err);
            }
        }
    }

    Ok(output)
}

#[instrument(skip(rpc_provider, time_price_bar_store, indexed_block_message_sender))]
async fn index_blocks<T, P>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
    indexed_block_message_sender: Sender<IndexedBlockMessage>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    for range_start_block_number in
        (start_block_number..=end_block_number).step_by(BLOCK_RANGE_STEP_BY as usize)
    {
        let range_end_block_number = min(
            range_start_block_number + BLOCK_RANGE_STEP_BY,
            end_block_number,
        );

        let parsed_blocks = get_parsed_blocks(
            Arc::clone(&rpc_provider),
            range_start_block_number,
            range_end_block_number,
        )
        .await?;

        for parsed_block in parsed_blocks.into_values() {
            time_price_bar_store
                .insert_block(Arc::clone(&rpc_provider), &parsed_block)
                .await?;

            let (indexed_block_message, ack_receiver) =
                IndexedBlockMessage::from_block_with_ack(&parsed_block);

            indexed_block_message_sender
                .send(indexed_block_message)
                .await?;

            // because this is a backfill, we wait for the receiver to fully process
            // the indexed block
            ack_receiver.await?;
        }

        debug!(
            range_start_block_number = range_start_block_number,
            range_end_block_number = range_end_block_number,
            "indexed block range step"
        );
    }

    Ok(())
}

impl<T, P> Indexer<T, P> for BlockRangeIndexer
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    fn subscribe(
        &mut self,
        rpc_provider: &Arc<RpcProvider<T, P>>,
    ) -> Receiver<IndexedBlockMessage> {
        let (indexed_block_message_sender, indexed_block_message_receiver) = channel(1);

        let start_block_number = self.start_block_number;
        let end_block_number = self.end_block_number;

        let rpc_provider = Arc::clone(rpc_provider);
        let time_price_bar_store = Arc::clone(&self.time_price_bar_store);

        let index_handle = tokio::spawn(async move {
            index_blocks(
                rpc_provider,
                time_price_bar_store,
                start_block_number,
                end_block_number,
                indexed_block_message_sender,
            )
            .await
            .inspect_err(|err| {
                error!(
                    start_block_number = start_block_number,
                    end_block_number = end_block_number,
                    "index_blocks failed: {:?}",
                    err
                );
            })
        });

        self.index_handle = Some(index_handle);

        indexed_block_message_receiver
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

        let parsed_blocks = get_parsed_blocks(rpc_provider, 13868901, 13868911).await?;

        assert_eq!(parsed_blocks.len(), 11);
        assert!(parsed_blocks.contains_key(&13868901));

        Ok(())
    }
}
