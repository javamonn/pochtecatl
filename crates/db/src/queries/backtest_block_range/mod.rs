use crate::primitives::U64;
use rusqlite::{named_params, Transaction};

pub struct BacktestBlockRange {
    pub start_timestamp: U64,
    pub end_timestamp: U64,
}

impl BacktestBlockRange {
    pub fn query_by_backtest_id(tx: &Transaction, backtest_id: i64) -> eyre::Result<Self> {
        tx.prepare_cached(include_str!("./query_by_backtest_id.sql"))?
            .query_row(named_params! { ":backtest_id": backtest_id }, |r| {
                Self::try_from(r)
            })
            .map_err(Into::into)
    }
}

impl<'stmt> TryFrom<&rusqlite::Row<'stmt>> for BacktestBlockRange {
    type Error = rusqlite::Error;

    fn try_from(row: &rusqlite::Row<'stmt>) -> rusqlite::Result<Self> {
        Ok(Self {
            start_timestamp: row.get(0)?,
            end_timestamp: row.get(1)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::BacktestBlockRange;

    use crate::{connect as connect_db, BlockModel, NewBacktestModel};

    use eyre::Result;

    #[test]
    pub fn test_query_by_backtest_id() -> Result<()> {
        let pool = connect_db(&String::from(":memory:")).unwrap();
        let mut conn = pool.get().unwrap();

        let backtest = NewBacktestModel::new(1, 2);
        let tx = conn.transaction()?;

        let backtest_id = backtest.insert(&tx).unwrap();
        BlockModel::new(1, 1, serde_json::json!({})).insert(&tx)?;
        BlockModel::new(2, 2, serde_json::json!({})).insert(&tx)?;
        BlockModel::new(3, 3, serde_json::json!({})).insert(&tx)?;

        let query = BacktestBlockRange::query_by_backtest_id(&tx, backtest_id).unwrap();

        assert_eq!(query.start_timestamp.0, 1);
        assert_eq!(query.end_timestamp.0, 2);

        Ok(())
    }
}
