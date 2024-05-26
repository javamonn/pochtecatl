use crate::primitives::{AppError, AppJson, AppState};
use pochtecatl_db::{BacktestBlockRangeQuery, BacktestPairQuery};

use alloy::primitives::Address;
use axum::extract::{Path, State};
use serde::Serialize;

#[derive(Serialize)]
pub struct BacktestPairQueryItem {
    pair_address: Address,
    trade_count: u64,
}

impl From<BacktestPairQuery> for BacktestPairQueryItem {
    fn from(backtest_pair: BacktestPairQuery) -> Self {
        Self {
            pair_address: backtest_pair.pair_address.into(),
            trade_count: backtest_pair.trade_count.into(),
        }
    }
}

#[derive(Serialize)]
pub struct Response {
    pairs: Vec<BacktestPairQueryItem>,
    backtest_start_at: u64,
    backtest_end_at: u64,
}

impl Response {
    pub fn new(
        pairs: Vec<BacktestPairQueryItem>,
        backtest_start_at: u64,
        backtest_end_at: u64,
    ) -> Self {
        Self {
            pairs,
            backtest_start_at,
            backtest_end_at,
        }
    }
}

pub async fn handler(
    Path(backtest_id): Path<i64>,
    State(app_state): State<AppState>,
) -> eyre::Result<AppJson<Response>, AppError> {
    let (backtest_pair_items, backtest_block_range) = {
        let mut db_conn = app_state.db().get()?;
        let tx = db_conn.transaction()?;
        let backtest_pair_items = BacktestPairQuery::query_by_backtest_id(&tx, backtest_id)?;
        let backtest_block_range = BacktestBlockRangeQuery::query_by_backtest_id(&tx, backtest_id)?;
        tx.rollback()?;

        (backtest_pair_items, backtest_block_range)
    };

    Ok(AppJson(Response::new(
        backtest_pair_items
            .into_iter()
            .map(BacktestPairQueryItem::from)
            .collect(),
        backtest_block_range.start_timestamp.into(),
        backtest_block_range.end_timestamp.into(),
    )))
}
