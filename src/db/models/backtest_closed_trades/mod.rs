use crate::trade_controller::TradeMetadata;

use eyre::Result;
use fallible_iterator::FallibleIterator;
use rusqlite::{named_params, Transaction};

pub struct BacktestClosedTrade {
    pub id: i64,
    pub backtest_id: i64,
    pub open_trade_metadata: serde_json::Value,
    pub close_trade_metadata: serde_json::Value,
}

pub struct NewBacktestClosedTrade {
    pub backtest_id: i64,
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
}

impl NewBacktestClosedTrade {
    pub fn new(
        backtest_id: i64,
        open_trade_metadata: TradeMetadata,
        close_trade_metadata: TradeMetadata,
    ) -> Self {
        Self {
            backtest_id,
            open_trade_metadata: serde_json::to_value(open_trade_metadata).unwrap(),
            close_trade_metadata: serde_json::to_value(close_trade_metadata).unwrap(),
        }
    }

    pub fn insert(self, tx: &Transaction) -> Result<()> {
        tx.prepare_cached(include_str!("./insert.sql"))?
            .execute(named_params! {
                ":backtest_id": self.backtest_id,
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
            open_trade_metadata: row.get(2)?,
            close_trade_metadata: row.get(3)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::NewBacktestClosedTrade;
    use crate::db::{connect as connect_db, models::BacktestClosedTrade};

    use eyre::Result;

    #[test]
    pub fn test_insert() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let trade = NewBacktestClosedTrade {
            backtest_id: 1,
            open_trade_metadata: serde_json::json!({}),
            close_trade_metadata: serde_json::json!({}),
        };

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
            NewBacktestClosedTrade {
                backtest_id: 1,
                open_trade_metadata: serde_json::json!({}),
                close_trade_metadata: serde_json::json!({}),
            },
            NewBacktestClosedTrade {
                backtest_id: 1,
                open_trade_metadata: serde_json::json!({}),
                close_trade_metadata: serde_json::json!({}),
            },
            NewBacktestClosedTrade {
                backtest_id: 2,
                open_trade_metadata: serde_json::json!({}),
                close_trade_metadata: serde_json::json!({}),
            },
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
}
