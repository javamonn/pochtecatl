mod abi;
mod config;
mod db;
mod indexer;
mod primitives;
mod providers;
mod strategies;
mod trade_controller;

use db::NewBacktestModel;
use indexer::{BlockRangeIndexer, Indexer};
use primitives::BlockId;
use providers::{rpc_provider::new_http_signer_provider, RpcProvider};
use strategies::{MomentumStrategy, StrategyExecutor};
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};
use trade_controller::TradeController;

use alloy::{network::Ethereum, providers::Provider, transports::Transport};

use eyre::{eyre, Result, WrapErr};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::{str::FromStr, sync::Arc};
use tracing::{info, instrument};

fn make_indexer<T, P>(
    rpc_provider: Arc<RpcProvider<T, P>>,
    db_pool: Arc<Pool<SqliteConnectionManager>>,
    start_block_id: &BlockId,
    end_block_id: &BlockId,
) -> Result<impl Indexer<T, P>>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    match (start_block_id, end_block_id) {
        (BlockId::BlockNumber(start), BlockId::BlockNumber(end)) => {
            if start < end {
                Ok(BlockRangeIndexer::new(
                    rpc_provider,
                    db_pool,
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
    let db_pool = Arc::new(db::connect(&config::DB_PATH)?);

    let mut indexer = make_indexer(
        Arc::clone(&rpc_provider),
        Arc::clone(&db_pool),
        &config::START_BLOCK_ID,
        &config::END_BLOCK_ID,
    )?;

    // Execute the indexer with the strategy executor
    indexer
        .exec(StrategyExecutor::new(
            Arc::clone(&trade_controller),
            Box::new(MomentumStrategy::new()),
        ))
        .await?;

    // wait for pending positions to settle
    trade_controller
        .pending_handle()
        .await
        .with_context(|| "pending_handle failed")?;

    // If backtesting, persist the trades for later inspection
    if *config::IS_BACKTEST {
        let mut conn = db_pool.get()?;
        let tx = conn.transaction()?;
        let backtest_id = NewBacktestModel::new().insert(&tx)?;
        trade_controller.insert_backtest_closed_trades(&tx, backtest_id)?;
    }

    info!("complete");

    Ok(())
}
