use super::{PendingTimePriceBar, ResolutionTimestamp, TimePriceBar, TimePriceBarData};

use alloy::primitives::BlockNumber;

use eyre::{eyre, Result};
use std::collections::BTreeMap;

pub struct TimePriceBars {
    data: BTreeMap<ResolutionTimestamp, TimePriceBar>,

    // How many historical TimePriceBars to retain. Once exceeded, oldest are
    // pruned first.
    retention_count: u64,

    // The last resolution timestamp with inserted (i.e. non-padded) data
    last_inserted_timestamp_with_data: Option<ResolutionTimestamp>,
}

impl TimePriceBars {
    pub fn new(retention_count: u64) -> Self {
        Self {
            data: BTreeMap::new(),
            retention_count,
            last_inserted_timestamp_with_data: None,
        }
    }

    pub fn get_data_range(
        &self,
        start_resolution_timestamp: &ResolutionTimestamp,
        end_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<Vec<(&ResolutionTimestamp, &TimePriceBarData)>> {
        let mut output = Vec::new();
        for (timestamp, time_price_bar) in self
            .data
            .range(start_resolution_timestamp..=end_resolution_timestamp)
        {
            match time_price_bar.get_data() {
                Some(data) => {
                    output.push((timestamp, data));
                }
                None => {
                    return Err(eyre!(
                        "Expected TimePriceBar to have data, but found none at {:?}, ",
                        timestamp
                    ))
                }
            }
        }

        Ok(output)
    }

    fn prune_to_retention_count(&mut self) {
        while self.data.len() > self.retention_count as usize {
            let _ = self.data.pop_first();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn is_stale(&self) -> bool {
        match (
            self.last_inserted_timestamp_with_data,
            self.data.first_key_value(),
        ) {
            (Some(last_inserted_timestamp_with_data), Some((first_retained_timestamp, _))) => {
                last_inserted_timestamp_with_data < *first_retained_timestamp
            }
            _ => false,
        }
    }

    pub fn prune_to_reorged_block_number(
        &mut self,
        reorged_block_number: BlockNumber,
    ) -> Result<()> {
        while let Some(mut time_price_bar_entry) = self.data.last_entry() {
            match time_price_bar_entry.get_mut() {
                TimePriceBar::Pending(time_price_bar) => {
                    match (
                        time_price_bar.start_block_number(),
                        time_price_bar.end_block_number(),
                    ) {
                        (Some(start_block_number), _)
                            if start_block_number >= &reorged_block_number =>
                        {
                            // time price bar holds a whole reorged range, remove the entry
                            time_price_bar_entry.remove_entry();
                        }
                        (_, Some(end_block_number))
                            if end_block_number >= &reorged_block_number =>
                        {
                            // prune the block price bar data within this time price bar on the
                            // boundary, then we're done
                            time_price_bar.prune_to_reorged_block_number(reorged_block_number);
                        }
                        (Some(_), Some(_)) => break,
                        (None, None) => {
                            return Err(eyre!("Attempted to prune into empty Pending TimePriceBar"))
                        }
                        (None, Some(_)) | (Some(_), None) => unreachable!(),
                    }
                }
                TimePriceBar::Finalized(time_price_bar) => {
                    if time_price_bar.end_block_number >= reorged_block_number {
                        return Err(eyre!("Attempted to prune into Finalized TimePriceBar"));
                    } else {
                        // Likely hit this after removing a whole pending entry
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn insert_data(
        &mut self,
        block_resolution_timestamp: ResolutionTimestamp,
        block_number: BlockNumber,
        data: TimePriceBarData,
    ) -> Result<()> {
        let time_price_bar = self
            .data
            .entry(block_resolution_timestamp.clone())
            .or_insert_with(|| TimePriceBar::Pending(PendingTimePriceBar::new()));

        time_price_bar.insert(block_number, data)?;

        // Update last inserted timestamp with data
        match self.last_inserted_timestamp_with_data {
            None => self.last_inserted_timestamp_with_data = Some(block_resolution_timestamp),
            Some(timestamp) if timestamp < block_resolution_timestamp => {
                self.last_inserted_timestamp_with_data = Some(block_resolution_timestamp)
            }
            _ => {}
        };

        self.prune_to_retention_count();

        Ok(())
    }

    pub fn finalize_range(
        &mut self,
        start_resolution_timestamp: &ResolutionTimestamp,
        end_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<()> {
        for (_, price_bar) in self
            .data
            .range_mut(start_resolution_timestamp..=end_resolution_timestamp)
        {
            match price_bar.into_finalized() {
                Ok(finalized_price_bar) => *price_bar = finalized_price_bar,
                Err(err) => {
                    log::warn!("Failed to finalize price bar: {:?}", err)
                }
            }
        }

        Ok(())
    }

    pub fn pad_for_block(
        &mut self,
        block_number: &BlockNumber,
        block_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<()> {
        let previous_block_price_bar_data = self.data
        .last_key_value()
        .ok_or_else(|| {
            eyre!("Expected time price bars with entries, but found none")
        })
        .and_then(|(_, time_price_bar)| match time_price_bar {
            TimePriceBar::Finalized(ref price_bar) => {
                if price_bar.end_block_number + 1 == *block_number {
                    Ok(price_bar.data.clone())
                } else {
                    Err(eyre!(
                        "Expected contiguous time price bars, but Finalized is disjoint: end_block_number {}, block_number {}",
                        price_bar.end_block_number,
                        block_number
                    ))
                }
            }
            TimePriceBar::Pending(ref price_bar) => {
                match price_bar.block_price_bars.last_key_value() {
                    Some((last_block_number, block_price_bar)) if last_block_number + 1 == *block_number => {
                        Ok(block_price_bar.clone())
                    }
                    _ => Err(eyre!("Expected contiguous time price bars, but last Pending is disjoint or missing"))

                }
            }
        })?;

        let time_price_bar = self
            .data
            .entry(block_resolution_timestamp.clone())
            .or_insert_with(|| TimePriceBar::Pending(PendingTimePriceBar::new()));

        time_price_bar.insert(*block_number, previous_block_price_bar_data)?;

        self.prune_to_retention_count();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::indexer::{Resolution, ResolutionTimestamp, TimePriceBarData};

    use super::TimePriceBars;

    use eyre::Result;
    use fraction::GenericFraction;

    #[test]
    pub fn test_insert_data() -> Result<()> {
        let mut time_price_bars = TimePriceBars::new(2);

        let mock_timestamp = ResolutionTimestamp::from_timestamp(100, &Resolution::FiveMinutes);

        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );

        // test initial insert
        time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
        let data = time_price_bars.get_data_range(&mock_timestamp, &mock_timestamp)?;
        assert_eq!(data.len(), 1);
        assert_eq!(data[0], (&mock_timestamp, &mock_data));

        // test prune to retention count
        let next_ts = mock_timestamp.next(&Resolution::FiveMinutes);
        time_price_bars.insert_data(next_ts, 2_u64, mock_data.clone())?;
        let last_ts = next_ts.next(&Resolution::FiveMinutes);
        time_price_bars.insert_data(last_ts, 3_u64, mock_data.clone())?;

        let data = time_price_bars.get_data_range(&mock_timestamp, &last_ts)?;
        assert_eq!(data.len(), 2);
        assert_eq!(data[0], (&next_ts, &mock_data));

        Ok(())
    }

    #[test]
    pub fn test_prune_to_reorged_block_number() -> Result<()> {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(100, &Resolution::FiveMinutes);
        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );

        // test the base case of pending block with partial block data
        {
            let mut time_price_bars = TimePriceBars::new(5);
            time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
            time_price_bars.insert_data(mock_timestamp, 2_u64, mock_data.clone())?;
            time_price_bars.insert_data(mock_timestamp, 3_u64, mock_data.clone())?;
            time_price_bars.prune_to_reorged_block_number(2_u64)?;
            let data = time_price_bars.get_data_range(&mock_timestamp, &mock_timestamp)?;
            assert_eq!(data.len(), 1);
        }

        // Alternate case of removing a whole time price bar
        {
            let mut time_price_bars = TimePriceBars::new(5);
            time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
            time_price_bars.insert_data(
                mock_timestamp.next(&Resolution::FiveMinutes),
                2_u64,
                mock_data.clone(),
            )?;
            time_price_bars.insert_data(
                mock_timestamp.next(&Resolution::FiveMinutes),
                3_u64,
                mock_data.clone(),
            )?;
            time_price_bars.prune_to_reorged_block_number(2_u64)?;
            let data = time_price_bars.get_data_range(&mock_timestamp, &mock_timestamp)?;
            assert_eq!(data.len(), 1);
        }

        Ok(())
    }

    #[test]
    pub fn test_pad_for_block() -> Result<()> {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(100, &Resolution::FiveMinutes);

        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );

        // Test padding from pending block
        {
            let mut time_price_bars = TimePriceBars::new(5);
            time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
            time_price_bars
                .pad_for_block(&2_u64, &mock_timestamp.next(&Resolution::FiveMinutes))?;
            let data = time_price_bars.get_data_range(
                &mock_timestamp,
                &mock_timestamp.next(&Resolution::FiveMinutes),
            )?;
            assert_eq!(data.len(), 2);
        }

        // Test padding from finalized block
        {
            let mut time_price_bars = TimePriceBars::new(5);
            time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
            time_price_bars.finalize_range(&mock_timestamp, &mock_timestamp)?;
            time_price_bars
                .pad_for_block(&2_u64, &mock_timestamp.next(&Resolution::FiveMinutes))?;
            let data = time_price_bars.get_data_range(
                &mock_timestamp,
                &mock_timestamp.next(&Resolution::FiveMinutes),
            )?;
            assert_eq!(data.len(), 2);
        }

        Ok(())
    }

    #[test]
    pub fn test_finalize_range() -> Result<()> {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(100, &Resolution::FiveMinutes);
        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );
        let mut time_price_bars = TimePriceBars::new(5);
        time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
        time_price_bars.finalize_range(&mock_timestamp, &mock_timestamp)?;
        let data = time_price_bars.get_data_range(&mock_timestamp, &mock_timestamp)?;
        assert_eq!(data.len(), 1);

        Ok(())
    }
}
