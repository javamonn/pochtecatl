use crate::primitives::{FixedBytes, U64};
use alloy::primitives::Address;

use fallible_iterator::FallibleIterator;
use rusqlite::{named_params, Transaction};

use eyre::Result;

#[derive(Debug, Clone)]
pub struct BacktestTimePriceBar {
    pub pair_address: FixedBytes<20>,
    pub resolution: U64,
    pub resolution_ts: U64,
    pub data: serde_json::Value,
}

impl BacktestTimePriceBar {
    pub fn new(
        pair_address: Address,
        resolution: u64,
        resolution_ts: u64,
        data: serde_json::Value,
    ) -> Self {
        Self {
            pair_address: pair_address.into(),
            resolution: resolution.into(),
            resolution_ts: resolution_ts.into(),
            data,
        }
    }

    pub fn insert(self, tx: &Transaction) -> Result<i64> {
        tx.prepare_cached(include_str!("./insert.sql"))?
            .insert(named_params! {
                ":pair_address": self.pair_address,
                ":resolution": self.resolution,
                ":resolution_ts": self.resolution_ts,
                ":data": self.data,
            })
            .map_err(Into::into)
    }

    pub fn query_by_pair_resolution_ts(
        tx: &Transaction,
        pair_address: Address,
        start_resolution_ts: u64,
        end_resolution_ts: u64,
    ) -> Result<Vec<Self>> {
        tx.prepare_cached(include_str!("./query_by_pair_resolution_ts.sql"))?
            .query(named_params! {
                ":pair_address": FixedBytes::from(pair_address),
                ":start_resolution_ts": U64::from(start_resolution_ts),
                ":end_resolution_ts": U64::from(end_resolution_ts),
            })?
            .map(|row| BacktestTimePriceBar::try_from(row))
            .collect()
            .map_err(Into::into)
    }
}

impl<'stmt> TryFrom<&rusqlite::Row<'stmt>> for BacktestTimePriceBar {
    type Error = rusqlite::Error;

    fn try_from(row: &rusqlite::Row<'stmt>) -> rusqlite::Result<Self> {
        Ok(Self {
            pair_address: row.get(0)?,
            resolution: row.get(1)?,
            resolution_ts: row.get(2)?,
            data: row.get(3)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::BacktestTimePriceBar;

    use crate::connect as connect_db;

    use alloy::primitives::Address;
    use eyre::Result;

    #[test]
    pub fn test_insert() -> Result<()> {
        let pool = connect_db(&String::from(":memory:")).unwrap();
        let mut conn = pool.get().unwrap();

        let time_price_bar = BacktestTimePriceBar::new(Address::ZERO, 1, 2, serde_json::json!({}));

        let tx = conn.transaction().unwrap();

        time_price_bar.clone().insert(&tx)?;

        // We should be able to re-insert without issue;
        time_price_bar.insert(&tx)?;

        tx.rollback()?;

        Ok(())
    }

    #[test]
    pub fn test_query_by_pair_resolution_ts() -> Result<()> {
        let pool = connect_db(&String::from(":memory:")).unwrap();
        let mut conn = pool.get().unwrap();

        let tx = conn.transaction().unwrap();

        BacktestTimePriceBar::new(Address::ZERO, 60, 60, serde_json::json!({})).insert(&tx)?;
        BacktestTimePriceBar::new(Address::ZERO, 60, 120, serde_json::json!({})).insert(&tx)?;
        BacktestTimePriceBar::new(Address::ZERO, 60, 180, serde_json::json!({})).insert(&tx)?;

        let results =
            BacktestTimePriceBar::query_by_pair_resolution_ts(&tx, Address::ZERO, 60, 120)?;

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].resolution_ts.0, 60);
        assert_eq!(results[1].resolution_ts.0, 120);

        Ok(())
    }
}
