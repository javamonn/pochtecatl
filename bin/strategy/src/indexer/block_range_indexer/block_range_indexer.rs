use crate::strategies::StrategyExecutor;

use pochtecatl_db::BlockModel;
use pochtecatl_primitives::{Block, Resolution, RpcProvider};

use super::{
    super::{time_price_bar_store::TimePriceBarStore, Indexer},
    BlockChunk, BlockChunkSource,
};

use alloy::{
    network::Ethereum, primitives::BlockNumber, providers::Provider, transports::Transport,
};

use eyre::{eyre, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::{
    cmp::min,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinSet,
};
use tracing::{debug, instrument, warn};

pub struct BlockRangeIndexer<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    db_pool: Arc<Pool<SqliteConnectionManager>>,
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
        db_pool: Arc<Pool<SqliteConnectionManager>>,
        start_block_number: B,
        end_block_number: B,
        is_backtest: bool,
    ) -> BlockRangeIndexer<T, P> {
        BlockRangeIndexer {
            rpc_provider,
            db_pool,
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

const BLOCK_PARSER_CHUNK_SIZE: u64 = 100;
const BLOCK_PARSER_CONCURRENCY: u64 = 50;
const PARSED_BLOCK_CHANNEL_CAPACITY: usize = 360;

async fn fetch_block_chunks_task<T, P>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    db_pool: Arc<Pool<SqliteConnectionManager>>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
    block_chunk_sender: Sender<BlockChunk>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    let block_number_chunk_iter = Arc::new(Mutex::new(
        (start_block_number..=end_block_number).step_by((BLOCK_PARSER_CHUNK_SIZE + 1) as usize),
    ));

    let mut subtasks = JoinSet::new();
    for _ in 0..BLOCK_PARSER_CONCURRENCY {
        let block_number_chunk_iter = Arc::clone(&block_number_chunk_iter);
        let block_chunk_sender = block_chunk_sender.clone();
        let rpc_provider = Arc::clone(&rpc_provider);
        let db_pool = Arc::clone(&db_pool);

        subtasks.spawn(async move {
            loop {
                match {
                    let mut iter = block_number_chunk_iter.lock().unwrap();
                    iter.next()
                } {
                    None => return Ok(()),
                    Some(chunk_start_block_number) => {
                        let chunk_end_block_number = min(
                            chunk_start_block_number + BLOCK_PARSER_CHUNK_SIZE,
                            end_block_number,
                        );

                        match BlockChunk::fetch(&rpc_provider, &db_pool, chunk_start_block_number, chunk_end_block_number)
                            .await
                        {
                            Ok(block_chunk) => match block_chunk_sender.send(block_chunk).await {
                                Ok(_) => {},
                                Err(e) => return Err(eyre!("block_parser_task failed due to block_chunk_sender error: {:?}", e)), 
                            },
                            Err(e) => return Err(eyre!("block_parser_task failed due to get_parsed_blocks error: {:?}", e)), 
                        }
                    }
                }
            }
        });
    }

    while let Some(result) = subtasks.join_next().await {
        result??;
    }

    Ok(())
}

async fn execute_strategy_for_block<T, P>(
    parsed_block: Block,
    rpc_provider: Arc<RpcProvider<T, P>>,
    strategy_executor: Arc<StrategyExecutor<T, P>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    time_price_bar_store
        .insert_block(Arc::clone(&rpc_provider), &parsed_block)
        .await?;

    strategy_executor
        .on_indexed_block_message(parsed_block.into(), &time_price_bar_store)
        .await?;

    Ok(())
}

#[instrument(skip_all)]
async fn handle_block_chunk_task<T, P>(
    mut parsed_block_chunk_receiver: Receiver<BlockChunk>,
    db_provider: &Pool<SqliteConnectionManager>,
    rpc_provider: Arc<RpcProvider<T, P>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
    strategy_executor: Arc<StrategyExecutor<T, P>>,
    start_block_number: BlockNumber,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    let mut next_block_number = start_block_number;
    let mut chunk_buffer: BTreeMap<BlockNumber, BlockChunk> = BTreeMap::new();

    while let Some(parsed_block_chunk) = parsed_block_chunk_receiver.recv().await {
        // Write the chunk to DB if required.
        if matches!(parsed_block_chunk.source, BlockChunkSource::Rpc) {
            let mut conn = db_provider.get()?;
            let tx = conn.transaction()?;

            for block in parsed_block_chunk.data.iter() {
                BlockModel::from(block).insert(&tx)?;
            }

            tx.commit()?;
        }

        // If the chunk is the next chunk we're looking for, execute the strategy for
        // it and all buffered contiguous chunks. Otherwise, buffer the chunk.
        match (
            parsed_block_chunk.data.first().map(|b| b.block_number),
            parsed_block_chunk.data.last().map(|b| b.block_number),
        ) {
            (Some(chunk_start_block_number), Some(chunk_end_block_number))
                if chunk_start_block_number == next_block_number =>
            {
                // process the chunk
                for block in parsed_block_chunk.data {
                    execute_strategy_for_block(
                        block,
                        Arc::clone(&rpc_provider),
                        Arc::clone(&strategy_executor),
                        Arc::clone(&time_price_bar_store),
                    )
                    .await?
                }
                debug!(
                    chunk_start_block_number = chunk_start_block_number,
                    chunk_end_block_number = chunk_end_block_number,
                    "Processed block chunk"
                );
                next_block_number = chunk_end_block_number + 1;

                // process any buffered blocks that fulfill the chunk
                while chunk_buffer
                    .first_key_value()
                    .map(|(chunk_start_block_number, _)| {
                        *chunk_start_block_number == next_block_number
                    })
                    .unwrap_or(false)
                {
                    let (block_number, parsed_block_chunk) = chunk_buffer.pop_first().unwrap();
                    let chunk_end_block_number = parsed_block_chunk
                        .data
                        .last()
                        .map(|b| b.block_number)
                        .unwrap_or(block_number);
                    for block in parsed_block_chunk.data {
                        execute_strategy_for_block(
                            block,
                            Arc::clone(&rpc_provider),
                            Arc::clone(&strategy_executor),
                            Arc::clone(&time_price_bar_store),
                        )
                        .await?
                    }
                    debug!(
                        chunk_start_block_number = chunk_start_block_number,
                        chunk_end_block_number = chunk_end_block_number,
                        "Processed block chunk"
                    );
                    next_block_number = chunk_end_block_number + 1;
                }
            }
            (Some(chunk_start_block_number), Some(_)) => {
                // buffer the block chunk
                chunk_buffer.insert(chunk_start_block_number, parsed_block_chunk);
            }
            (None, _) | (_, None) => {
                warn!("Received empty parsed_block_chunk");
            }
        }
    }

    Ok(())
}

impl<T, P> Indexer<T, P> for BlockRangeIndexer<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    async fn exec(&mut self, strategy_executor: StrategyExecutor<T, P>) -> Result<()> {
        let (block_chunk_sender, block_chunk_receiver) = channel(PARSED_BLOCK_CHANNEL_CAPACITY);

        let strategy_executor_join_handle = {
            let rpc_provider = Arc::clone(&self.rpc_provider);
            let time_price_bar_store = Arc::clone(&self.time_price_bar_store);
            let start_block_number = self.start_block_number;
            let strategy_executor = Arc::new(strategy_executor);
            let db_pool = Arc::clone(&self.db_pool);

            tokio::spawn(async move {
                handle_block_chunk_task(
                    block_chunk_receiver,
                    &db_pool,
                    rpc_provider,
                    time_price_bar_store,
                    strategy_executor,
                    start_block_number,
                )
                .await
            })
        };
        let parser_join_handle = {
            let rpc_provider = Arc::clone(&self.rpc_provider);
            let start_block_number = self.start_block_number;
            let end_block_number = self.end_block_number;
            let db_pool = Arc::clone(&self.db_pool);

            tokio::spawn(async move {
                fetch_block_chunks_task(
                    rpc_provider,
                    db_pool,
                    start_block_number,
                    end_block_number,
                    block_chunk_sender,
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
}
