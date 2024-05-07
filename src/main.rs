mod abi;
mod config;
mod indexer;
mod primitives;
mod providers;
mod strategies;
mod trade_controller;

use indexer::{BlockRangeIndexer, Indexer};
use primitives::BlockId;
use providers::{rpc_provider::new_http_signer_provider, RpcProvider};
use strategies::{UniswapV2MomentumStrategy, UniswapV2StrategyExecuctor};
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};
use trade_controller::TradeController;

use alloy::{network::Ethereum, providers::Provider, transports::Transport};

use eyre::{eyre, Result, WrapErr};
use std::{str::FromStr, sync::Arc};
use tracing::{info, instrument};

fn make_indexer<T, P>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    start_block_id: &BlockId,
    end_block_id: &BlockId,
) -> Result<impl Indexer>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    match (start_block_id, end_block_id) {
        (BlockId::BlockNumber(start), BlockId::BlockNumber(end)) => {
            if start < end {
                Ok(BlockRangeIndexer::new(
                    rpc_provider,
                    start.clone(),
                    end.clone(),
                    *config::IS_BACKTEST,
                ))
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
#[instrument]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_str(&config::RUST_LOG).unwrap_or_default())
        .with_span_events(FmtSpan::CLOSE)
        .init();

    info!(
        rust_log = *config::RUST_LOG,
        start_block_id = config::START_BLOCK_ID.to_string(),
        end_block_id = config::END_BLOCK_ID.to_string(),
        "start"
    );

    let rpc_provider = Arc::new(new_http_signer_provider(&config::RPC_URL, None).await?);
    let trade_controller = Arc::new(TradeController::new(Arc::clone(&rpc_provider)));

    let mut indexer = make_indexer(
        Arc::clone(&rpc_provider),
        &config::START_BLOCK_ID,
        &config::END_BLOCK_ID,
    )?;

    // Execute the indexer with the strategy executor
    indexer
        .exec(
            UniswapV2StrategyExecuctor::<UniswapV2MomentumStrategy, _, _>::with_momentum_strategy(
                Arc::clone(&trade_controller),
            ),
        )
        .await?;

    // wait for pending positions to settle
    trade_controller
        .pending_handle()
        .await
        .with_context(|| "pending_handle failed")?;

    info!("complete");

    Ok(())
}
