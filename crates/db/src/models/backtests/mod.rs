use crate::primitives::U64;

use alloy::primitives::BlockNumber;
use eyre::Result;
use fallible_iterator::FallibleIterator;
use rusqlite::{named_params, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Backtest {
    pub id: i64,
    pub created_at: U64,
    pub start_block_number: U64,
    pub end_block_number: U64,
}

impl Backtest {
    pub fn query_all(tx: &Transaction) -> Result<Vec<Self>> {
        tx.prepare_cached(include_str!("./query_all.sql"))?
            .query([])?
            .map(|row| Self::try_from(row))
            .collect()
            .map_err(Into::into)
    }
}

impl<'stmt> TryFrom<&rusqlite::Row<'stmt>> for Backtest {
    type Error = rusqlite::Error;

    fn try_from(row: &rusqlite::Row<'stmt>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            created_at: row.get(1)?,
            start_block_number: row.get(2)?,
            end_block_number: row.get(3)?,
        })
    }
}

pub struct NewBacktest {
    pub created_at: U64,
    pub start_block_number: U64,
    pub end_block_number: U64,
}

impl NewBacktest {
    pub fn new(start_block_number: BlockNumber, end_block_number: BlockNumber) -> Self {
        Self {
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .into(),
            start_block_number: start_block_number.into(),
            end_block_number: end_block_number.into(),
        }
    }

    pub fn insert(self, tx: &Transaction) -> Result<i64> {
        tx.prepare_cached(include_str!("./insert.sql"))?
            .insert(named_params! {
                ":created_at": self.created_at,
                ":start_block_number": self.start_block_number,
                ":end_block_number": self.end_block_number,
            })
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::{Backtest, NewBacktest};
    use crate::connect as connect_db;
    use eyre::Result;

    #[test]
    pub fn test_insert() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let backtest = NewBacktest::new(1, 2);
        let id = {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            let id = backtest.insert(&tx)?;
            tx.commit()?;
            id
        };

        assert_eq!(id, 1);

        Ok(())
    }

    #[test]
    pub fn test_query_all() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;
        let mut conn = pool.get()?;

        // Insert backtests
        {
            let backtests = vec![
                NewBacktest::new(1, 2),
                NewBacktest::new(3, 4),
                NewBacktest::new(5, 6),
            ];

            let tx = conn.transaction()?;
            for backtest in backtests {
                backtest.insert(&tx)?;
            }
            tx.commit()?;
        }

        // Query backtests
        {
            let tx = conn.transaction()?;
            let backtests = Backtest::query_all(&tx)?;
            assert_eq!(backtests.len(), 3);
            assert_eq!(backtests[0].id, 1);
            assert_eq!(backtests[0].start_block_number.0, 1);
            assert_eq!(backtests[0].end_block_number.0, 2);
            tx.rollback()?;
        }

        Ok(())
    }
}
