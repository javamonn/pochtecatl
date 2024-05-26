use crate::primitives::{FixedBytes, U64};

use fallible_iterator::FallibleIterator;
use rusqlite::{named_params, Transaction};

pub struct BacktestPair {
    pub pair_address: FixedBytes<20>,
    pub trade_count: U64,
}

impl BacktestPair {
    pub fn query_by_backtest_id(tx: &Transaction, backtest_id: i64) -> eyre::Result<Vec<Self>> {
        tx.prepare_cached(include_str!("./query_by_backtest_id.sql"))?
            .query(named_params! { ":backtest_id": backtest_id })?
            .map(|row| Self::try_from(row))
            .collect()
            .map_err(Into::into)
    }
}

impl<'stmt> TryFrom<&rusqlite::Row<'stmt>> for BacktestPair {
    type Error = rusqlite::Error;

    fn try_from(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            pair_address: row.get(0)?,
            trade_count: row.get(1)?,
        })
    }
}

#[cfg(test)]
mod tests {

    use super::BacktestPair;
    use crate::{connect as connect_db, models::NewBacktestClosedTrade};

    use alloy::primitives::Address;

    use eyre::Result;

    pub fn test_query_by_backtest_id() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;
        let mut conn = pool.get()?;

        let trades = vec![
            NewBacktestClosedTrade::new(
                1,
                Address::repeat_byte(1),
                1,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
            NewBacktestClosedTrade::new(
                1,
                Address::repeat_byte(2),
                1,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
            NewBacktestClosedTrade::new(
                1,
                Address::repeat_byte(2),
                1,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
        ];

        {
            let tx = conn.transaction()?;
            for trade in trades.into_iter() {
                trade.insert(&tx)?;
            }
            tx.commit()?;
        }

        {
            let tx = conn.transaction()?;
            let results = BacktestPair::query_by_backtest_id(&tx, 1)?;

            assert_eq!(results.len(), 2);
            assert_eq!(results[0].pair_address.0, Address::repeat_byte(1).0);
            assert_eq!(results[0].trade_count.0, 1);
            assert_eq!(results[1].pair_address.0, Address::repeat_byte(2).0);
            assert_eq!(results[1].trade_count.0, 2);

            tx.rollback()?;
        }

        Ok(())
    }
}
