use crate::primitives::{AppError, AppJson, AppState};
use pochtecatl_db::BacktestModel;

use axum::extract::State;
use serde::Serialize;

#[derive(Serialize)]
pub struct BacktestItem {
    id: i64,
    created_at: u64,
    start_block_number: u64,
    end_block_number: u64,
}

impl From<BacktestModel> for BacktestItem {
    fn from(backtest: BacktestModel) -> Self {
        Self {
            id: backtest.id,
            created_at: backtest.created_at.into(),
            start_block_number: backtest.start_block_number.into(),
            end_block_number: backtest.end_block_number.into(),
        }
    }
}

type Response = Vec<BacktestItem>;

pub async fn handler(State(state): State<AppState>) -> eyre::Result<AppJson<Response>, AppError> {
    let backtest_items = {
        let mut db_conn = state.db().get()?;
        let tx = db_conn.transaction()?;
        let backtest_items = BacktestModel::query_all(&tx)?;
        tx.rollback()?;
        backtest_items
    };

    Ok(AppJson(
        backtest_items.into_iter().map(BacktestItem::from).collect(),
    ))
}
