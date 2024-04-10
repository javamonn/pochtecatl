mod abi;
mod config;
mod indexer;
mod primitives;
mod rpc_provider;
mod strategies;
mod trade_position_controller;

use indexer::BlockRangeIndexer;
use primitives::BlockId;
use rpc_provider::RpcProvider;
use strategies::UniswapV2MomentumStrategy;
use trade_position_controller::TradePositionController;

use eyre::{eyre, Result};
use std::sync::Arc;

fn make_price_indexer(
    start_block_id: &BlockId,
    end_block_id: &BlockId,
) -> Result<Box<dyn indexer::Indexer>> {
    match (start_block_id, end_block_id) {
        (BlockId::BlockNumber(start), BlockId::BlockNumber(end)) => {
            if start < end {
                Ok(Box::new(BlockRangeIndexer::new(start.clone(), end.clone())))
            } else {
                Err(eyre!(
                    "Failed to create BlockRangeIndexer due to invalid block numbers: start {}, end {}",
                    start,
                    end
                ))
            }
        }
        (BlockId::Latest, BlockId::Latest) => unimplemented!(),
        _ => Err(eyre!("Failed to create Indexer")),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    structured_logger::Builder::with_level(&config::RUST_LOG).init();

    log::info!(
        rust_log = *config::RUST_LOG,
        start_block_id = *config::START_BLOCK_ID,
        end_block_id = *config::END_BLOCK_ID;
        "start"
    );

    let rpc_provider = Arc::new(RpcProvider::new(&config::RPC_URL).await?);

    let mut price_indexer = make_price_indexer(&config::START_BLOCK_ID, &config::END_BLOCK_ID)?;
    let trade_position_controller =
        Arc::new(TradePositionController::new(Arc::clone(&rpc_provider)));

    let mut strategy = UniswapV2MomentumStrategy::new(
        rpc_provider.signer_address(),
        price_indexer.time_price_bar_store(),
        trade_position_controller,
    );

    // start the block indexer and strategy
    let indexed_block_message_receiver = price_indexer.subscribe(&rpc_provider);
    strategy.exec(indexed_block_message_receiver);

    // wait for the strategy to finish
    strategy.join().await
}
