use crate::primitives::{FixedBytes, U64};
use alloy::primitives::Address;

use eyre::Result;
use fallible_iterator::FallibleIterator;
use rusqlite::{named_params, Transaction};

pub struct BacktestClosedTrade {
    pub id: i64,
    pub pair_address: FixedBytes<20>,
    pub backtest_id: i64,
    pub close_trade_block_timestamp: U64,
    pub open_trade_metadata: serde_json::Value,
    pub close_trade_metadata: serde_json::Value,
}

pub struct NewBacktestClosedTrade {
    pub backtest_id: i64,
    pub close_trade_block_timestamp: U64,
    pub pair_address: FixedBytes<20>,
    pub open_trade_metadata: serde_json::Value,
    pub close_trade_metadata: serde_json::Value,
}

impl BacktestClosedTrade {
    pub fn query_by_backtest_id(tx: &Transaction, backtest_id: i64) -> Result<Vec<Self>> {
        tx.prepare_cached(include_str!("./query_by_backtest_id.sql"))?
            .query(named_params! {
                ":backtest_id": backtest_id,
            })?
            .map(|row| BacktestClosedTrade::try_from(row))
            .collect()
            .map_err(Into::into)
    }

    pub fn query_by_backtest_pair_timestamp(
        tx: &Transaction,
        backtest_id: i64,
        pair_address: Address,
        start_close_trade_block_timestamp: u64,
        end_close_trade_block_timestamp: u64,
    ) -> Result<Vec<Self>> {
        tx.prepare_cached(include_str!("./query_by_backtest_pair_timestamp.sql"))?
            .query(named_params! {
                ":backtest_id": backtest_id,
                ":pair_address": FixedBytes::from(pair_address),
                ":start_close_trade_block_timestamp": U64::from(start_close_trade_block_timestamp),
                ":end_close_trade_block_timestamp": U64::from(end_close_trade_block_timestamp),
            })?
            .map(|row| BacktestClosedTrade::try_from(row))
            .collect()
            .map_err(Into::into)
    }
}

impl NewBacktestClosedTrade {
    pub fn new(
        backtest_id: i64,
        pair_address: Address,
        close_trade_block_timestamp: u64,
        open_trade_metadata: serde_json::Value,
        close_trade_metadata: serde_json::Value,
    ) -> Self {
        Self {
            backtest_id,
            pair_address: pair_address.into(),
            close_trade_block_timestamp: close_trade_block_timestamp.into(),
            open_trade_metadata,
            close_trade_metadata,
        }
    }

    pub fn insert(self, tx: &Transaction) -> Result<()> {
        tx.prepare_cached(include_str!("./insert.sql"))?
            .execute(named_params! {
                ":backtest_id": self.backtest_id,
                ":pair_address": self.pair_address,
                ":close_trade_block_timestamp": self.close_trade_block_timestamp,
                ":open_trade_metadata": self.open_trade_metadata,
                ":close_trade_metadata": self.close_trade_metadata,
            })
            .map_err(Into::into)
            .and_then(|n| {
                if n == 1 {
                    Ok(())
                } else {
                    Err(eyre::eyre!("Unexpected number of rows inserted: {}", n))
                }
            })
    }
}

impl<'stmt> TryFrom<&rusqlite::Row<'stmt>> for BacktestClosedTrade {
    type Error = rusqlite::Error;

    fn try_from(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            backtest_id: row.get(1)?,
            pair_address: row.get(2)?,
            close_trade_block_timestamp: row.get(3)?,
            open_trade_metadata: row.get(4)?,
            close_trade_metadata: row.get(5)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BacktestClosedTrade, NewBacktestClosedTrade};
    use crate::connect as connect_db;

    use alloy::primitives::Address;
    use eyre::Result;

    #[test]
    pub fn test_insert() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let trade = NewBacktestClosedTrade::new(
            1,
            Address::ZERO,
            1,
            serde_json::json!({}),
            serde_json::json!({}),
        );

        {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            trade.insert(&tx)?;
            tx.commit()?;
        }

        Ok(())
    }

    #[test]
    pub fn test_query_by_backtest_id() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let trades = vec![
            NewBacktestClosedTrade::new(
                1,
                Address::ZERO,
                1,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
            NewBacktestClosedTrade::new(
                1,
                Address::ZERO,
                1,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
            NewBacktestClosedTrade::new(
                2,
                Address::ZERO,
                2,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
        ];

        {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            for trade in trades.into_iter() {
                trade.insert(&tx)?;
            }
            tx.commit()?;
        }

        {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            let trades = BacktestClosedTrade::query_by_backtest_id(&tx, 1)?;

            assert_eq!(trades.len(), 2);
            assert_eq!(trades[0].id, 1);
            assert_eq!(trades[1].id, 2);

            tx.rollback()?;
        }

        Ok(())
    }

    #[test]
    pub fn test_query_by_backtest_pair_timestamp() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        // Insert trade data
        {
            let trades = vec![
                NewBacktestClosedTrade::new(
                    1,
                    Address::ZERO,
                    1,
                    serde_json::json!({}),
                    serde_json::json!({}),
                ),
                NewBacktestClosedTrade::new(
                    2,
                    Address::ZERO,
                    2,
                    serde_json::json!({}),
                    serde_json::json!({}),
                ),
                NewBacktestClosedTrade::new(
                    2,
                    Address::ZERO,
                    3,
                    serde_json::json!({}),
                    serde_json::json!({}),
                ),
            ];
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            for trade in trades.into_iter() {
                trade.insert(&tx)?;
            }
            tx.commit()?;
        }

        {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            let trades =
                BacktestClosedTrade::query_by_backtest_pair_timestamp(&tx, 2, Address::ZERO, 2, 3)?;

            assert_eq!(trades.len(), 2);
            assert_eq!(trades[0].close_trade_block_timestamp.0, 2);
            assert_eq!(trades[1].close_trade_block_timestamp.0, 3);

            tx.rollback()?;
        }

        Ok(())
    }
}
