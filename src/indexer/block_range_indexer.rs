use crate::{abi::IUniswapV2Pair, rpc_provider::RpcProvider};

use super::{
    log_parser, time_price_bar_store::TimePriceBarStore, Block, IndexedBlockMetadata, Indexer,
    Resolution,
};

use alloy::{
    primitives::{BlockNumber, U256},
    rpc::types::eth::{Filter, Log as RpcLog},
    sol_types::SolEvent,
};

use eyre::Result;
use std::{
    cmp::min,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc,
    },
};
use tokio::task::JoinHandle;

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
    ) -> BlockRangeIndexer {
        BlockRangeIndexer {
            start_block_number: start_block_number.into(),
            end_block_number: end_block_number.into(),
            index_handle: None,
            time_price_bar_store: Arc::new(TimePriceBarStore::new(Resolution::FiveMinutes, 60)),
        }
    }
}

const BLOCK_RANGE_STEP_BY: u64 = 100;

async fn index_blocks(
    rpc_provider: Arc<RpcProvider>,
    time_price_bar_store: Arc<TimePriceBarStore>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
) -> Result<()> {
    for range_start_block_number in
        (start_block_number..=end_block_number).step_by(BLOCK_RANGE_STEP_BY as usize)
    {
        let range_end_block_number = min(
            range_start_block_number + BLOCK_RANGE_STEP_BY,
            end_block_number,
        );

        let filter = Filter::new()
            .from_block(range_start_block_number)
            .to_block(range_end_block_number)
            .event_signature(vec![
                IUniswapV2Pair::Sync::SIGNATURE_HASH,
                IUniswapV2Pair::Swap::SIGNATURE_HASH,
            ]);

        let (logs, block_headers) = tokio::join!(
            rpc_provider.get_logs(&filter),
            rpc_provider
                .get_block_headers_by_range(range_start_block_number, range_end_block_number)
        );

        let logs = logs.unwrap_or_else(|err| {
            log::error!(
                range_start_block_number = range_start_block_number,
                range_end_block_number = range_end_block_number;
                "get_logs failed: {:?}", err
            );
            Vec::new()
        });

        let mut block_log_start_idx = 0;
        let mut parsed_blocks = Vec::with_capacity(BLOCK_RANGE_STEP_BY as usize);

        // Parse the blocks
        for block_number in range_start_block_number..=range_end_block_number {
            let block_logs = {
                let wrapped_block_number = U256::from(block_number);
                let block_logs = logs
                    .iter()
                    .skip(block_log_start_idx)
                    .take_while(|log| {
                        log.block_number.is_some_and(|log_block_number| {
                            log_block_number == wrapped_block_number
                        })
                    })
                    .collect::<Vec<&RpcLog>>();
                block_log_start_idx += block_logs.len();

                block_logs
            };

            match block_headers.get(&block_number) {
                Some(header) => {
                    parsed_blocks.push(log_parser::parse(header, block_logs)?);
                }
                None => {
                    log::warn!("Expected header for block {} but found none", block_number);
                }
            }
        }

        for block in parsed_blocks.into_iter() {
            time_price_bar_store
                .insert_block(Arc::clone(&rpc_provider), &block)
                .await?;
        }
    }

    Ok(())
}

impl Indexer for BlockRangeIndexer {
    fn subscribe(&mut self, rpc_provider: &Arc<RpcProvider>) -> Receiver<IndexedBlockMetadata> {
        let (indexed_block_sender, indexed_block_receiver) = sync_channel(64);

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
            )
            .await
        });

        self.index_handle = Some(index_handle);

        indexed_block_receiver
    }
}
