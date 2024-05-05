mod abi;
mod config;
mod indexer;
mod primitives;
mod providers;
mod strategies;
mod trade_controller;

use indexer::BlockRangeIndexer;
use primitives::BlockId;
use providers::rpc_provider::new_http_signer_provider;
use strategies::{StrategyExecutor, UniswapV2MomentumStrategy, UniswapV2StrategyExecuctor};
use tracing_subscriber::EnvFilter;
use trade_controller::TradeController;

use alloy::{network::Ethereum, providers::Provider, transports::Transport};

use eyre::{eyre, Result, WrapErr};
use std::{str::FromStr, sync::Arc};
use tracing::info;

fn make_price_indexer<T, P>(
    start_block_id: &BlockId,
    end_block_id: &BlockId,
) -> Result<Box<dyn indexer::Indexer<T, P>>>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    match (start_block_id, end_block_id) {
        (BlockId::BlockNumber(start), BlockId::BlockNumber(end)) => {
            if start < end {
                Ok(Box::new(BlockRangeIndexer::new(
                    start.clone(),
                    end.clone(),
                    *config::IS_BACKTEST,
                )))
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
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_str(&config::RUST_LOG).unwrap_or_default())
        .init();

    info!(
        rust_log = *config::RUST_LOG,
        start_block_id = config::START_BLOCK_ID.to_string(),
        end_block_id = config::END_BLOCK_ID.to_string(),
        "start"
    );

    let rpc_provider = Arc::new(new_http_signer_provider(&config::RPC_URL, None).await?);

    let mut price_indexer = make_price_indexer(&config::START_BLOCK_ID, &config::END_BLOCK_ID)?;
    let trade_controller = Arc::new(TradeController::new(Arc::clone(&rpc_provider)));

    let mut strategy_executor =
        UniswapV2StrategyExecuctor::<UniswapV2MomentumStrategy, _, _>::with_momentum_strategy(
            price_indexer.time_price_bar_store(),
            Arc::clone(&trade_controller),
        );

    // start the block indexer and strategy
    let indexed_block_message_receiver = price_indexer.subscribe(&rpc_provider);
    strategy_executor.exec(indexed_block_message_receiver);

    info!("waiting for strategy to finish");

    // wait for the strategy to finish
    strategy_executor.join().await?;

    info!("waiting for pending positions to settle");

    // wait for pending positions to settle
    //
    trade_controller
        .pending_handle()
        .await
        .with_context(|| "pending_handle failed")?;

    info!("complete");

    Ok(())
}
