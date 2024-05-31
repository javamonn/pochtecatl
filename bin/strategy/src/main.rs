mod config;
mod indexer;
mod strategies;
mod trade_controller;

use pochtecatl_db::{connect, NewBacktestModel};
use pochtecatl_primitives::{new_http_signer_provider, BlockId, RpcProvider, Resolution};

use indexer::{BlockRangeIndexer, Indexer};

use strategies::{MomentumStrategy, StrategyExecutor};
use tracing_subscriber::EnvFilter;
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
                    Resolution::OneHour
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
    let file_appender =
        tracing_appender::rolling::hourly(config::LOG_DIR.clone(), "pochtecatl-strategy.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_str(&config::RUST_LOG).unwrap_or_default())
        .with_span_events(config::TRACING_SPAN_EVENTS.clone())
        .with_ansi(false)
        .with_writer(non_blocking)
        .init();

    info!(
        rust_log = *config::RUST_LOG,
        start_block_id = config::START_BLOCK_ID.to_string(),
        end_block_id = config::END_BLOCK_ID.to_string(),
        "start"
    );

    let rpc_provider = Arc::new(
        new_http_signer_provider(
            config::RPC_URL.clone(),
            &config::WALLET_PRIVATE_KEY,
            None,
            *config::IS_BACKTEST,
        )
        .await?,
    );
    let trade_controller = Arc::new(TradeController::new(Arc::clone(&rpc_provider)));
    let db_pool = Arc::new(connect(&config::DB_PATH)?);

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
    match (*config::START_BLOCK_ID, *config::END_BLOCK_ID) {
        (BlockId::BlockNumber(start_block_number), BlockId::BlockNumber(end_block_number)) => {
            let mut conn = db_pool.get()?;
            let tx = conn.transaction()?;

            let backtest_id =
                NewBacktestModel::new(start_block_number, end_block_number).insert(&tx)?;
            trade_controller.insert_backtest_closed_trades(&tx, backtest_id)?;

            tx.commit()?;
        }
        _ => info!("Live execution, not persisting trades"),
    }

    info!("complete");

    Ok(())
}
