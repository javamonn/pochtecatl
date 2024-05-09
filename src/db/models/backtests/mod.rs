use crate::db::primitives::U64;

use eyre::Result;
use rusqlite::{named_params, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Backtest {
    pub id: i64,
    pub timestamp: U64,
}

pub struct NewBacktest {
    pub timestamp: U64,
}

impl NewBacktest {
    pub fn new() -> Self {
        Self {
            timestamp: U64::from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
        }
    }

    pub fn insert(self, tx: &Transaction) -> Result<i64> {
        tx.prepare_cached(include_str!("./insert.sql"))?
            .insert(named_params! {
                ":timestamp": self.timestamp,
            })
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::NewBacktest;
    use crate::db::connect as connect_db;
    use eyre::Result;

    #[test]
    pub fn test_insert() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let backtest = NewBacktest::new();
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
}
