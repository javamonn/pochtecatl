use crate::primitives::U64;

use alloy::primitives::BlockNumber;

use eyre::Result;
use fallible_iterator::FallibleIterator;
use rusqlite::{named_params, Transaction};

#[derive(Debug, Clone)]
pub struct Block {
    pub number: U64,
    pub timestamp: U64,
    pub pair_ticks: serde_json::Value,
}

impl Block {
    pub fn new(number: BlockNumber, timestamp: u64, pair_ticks: serde_json::Value) -> Self {
        Self {
            number: number.into(),
            timestamp: timestamp.into(),
            pair_ticks,
        }
    }
}

impl Block {
    pub fn insert(self, tx: &Transaction) -> Result<()> {
        tx.prepare_cached(include_str!("./insert.sql"))?
            .execute(named_params! {
                ":number": self.number,
                ":timestamp": self.timestamp,
                ":pair_ticks": self.pair_ticks,
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

    pub fn query_by_number_range(
        tx: &Transaction,
        min_number: u64,
        max_number: u64,
    ) -> Result<Vec<Self>> {
        tx.prepare_cached(include_str!("./query_by_number_range.sql"))?
            .query(named_params! {
                ":min_number": min_number,
                ":max_number": max_number,
            })?
            .map(|row| Block::try_from(row))
            .collect()
            .map_err(Into::into)
    }

    pub fn query_by_timestamp_range(
        tx: &Transaction,
        min_timestamp: u64,
        max_timestamp: u64,
    ) -> Result<Vec<Self>> {
        tx.prepare_cached(include_str!("./query_by_timestamp_range.sql"))?
            .query(named_params! {
                ":min_timestamp": min_timestamp,
                ":max_timestamp": max_timestamp,
            })?
            .map(|row| Block::try_from(row))
            .collect()
            .map_err(Into::into)
    }
}

impl<'stmt> TryFrom<&rusqlite::Row<'stmt>> for Block {
    type Error = rusqlite::Error;

    fn try_from(row: &rusqlite::Row<'stmt>) -> rusqlite::Result<Self> {
        Ok(Self {
            number: row.get(0)?,
            timestamp: row.get(1)?,
            pair_ticks: row.get(2)?,
        })
    }
}

#[cfg(test)]
mod tests {

    use super::Block;
    use crate::connect as connect_db;
    use eyre::Result;

    #[test]
    pub fn test_insert() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let block = Block {
            number: 1.into(),
            timestamp: 1.into(),
            pair_ticks: serde_json::json!({ "foo": "bar" }),
        };

        {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            block.insert(&tx)?;
            tx.commit()?;
        }

        Ok(())
    }

    #[test]
    pub fn test_query_by_number_range() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let mut conn = pool.get()?;
        let tx = conn.transaction()?;

        // Insert test data
        let blocks = vec![
            Block {
                number: 1.into(),
                timestamp: 1.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
            Block {
                number: 2.into(),
                timestamp: 2.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
            Block {
                number: 3.into(),
                timestamp: 3.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
            Block {
                number: 4.into(),
                timestamp: 4.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
        ];
        for block in blocks.into_iter() {
            block.insert(&tx)?;
        }

        // test
        let blocks = Block::query_by_number_range(&tx, 2, 3)?;
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].number, 2.into());
        assert_eq!(blocks[1].number, 3.into());

        tx.rollback()?;

        Ok(())
    }

    #[test]
    pub fn test_query_by_timestamp_range() -> Result<()> {
        let pool = connect_db(&String::from(":memory:"))?;

        let mut conn = pool.get()?;
        let tx = conn.transaction()?;

        // Insert test data
        let blocks = vec![
            Block {
                number: 1.into(),
                timestamp: 1.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
            Block {
                number: 2.into(),
                timestamp: 2.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
            Block {
                number: 3.into(),
                timestamp: 3.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
            Block {
                number: 4.into(),
                timestamp: 4.into(),
                pair_ticks: serde_json::json!({ "foo": "bar" }),
            },
        ];
        for block in blocks.into_iter() {
            block.insert(&tx)?;
        }

        // test
        let blocks = Block::query_by_timestamp_range(&tx, 2, 3)?;
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].timestamp, 2.into());
        assert_eq!(blocks[1].timestamp, 3.into());

        tx.rollback()?;

        Ok(())
    }
}
