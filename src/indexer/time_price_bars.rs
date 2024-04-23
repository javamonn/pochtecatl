use super::{
    Indicators, PendingTimePriceBar, Resolution, ResolutionTimestamp, TimePriceBar,
    TimePriceBarData,
};

use alloy::primitives::BlockNumber;

use eyre::{eyre, Result};
use std::collections::BTreeMap;

pub struct TimePriceBars {
    data: BTreeMap<ResolutionTimestamp, TimePriceBar>,

    resolution: Resolution,

    // How many historical TimePriceBars to retain. Once exceeded, oldest are
    // pruned first.
    retention_count: u64,

    // The last resolution timestamp with inserted (i.e. non-padded) data
    last_inserted_timestamp_with_data: Option<ResolutionTimestamp>,
}

impl TimePriceBars {
    pub fn new(retention_count: u64, resolution: Resolution) -> Self {
        Self {
            data: BTreeMap::new(),
            retention_count,
            resolution,
            last_inserted_timestamp_with_data: None,
        }
    }

    pub fn data(&self) -> &BTreeMap<ResolutionTimestamp, TimePriceBar> {
        &self.data
    }

    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }

    pub fn time_price_bar(&self, timestamp: &ResolutionTimestamp) -> Option<&TimePriceBar> {
        self.data.get(timestamp)
    }

    #[cfg(test)]
    pub fn time_price_bar_range(
        &self,
        start_resolution_timestamp: &ResolutionTimestamp,
        end_resolution_timestamp: &ResolutionTimestamp,
    ) -> Vec<(&ResolutionTimestamp, &TimePriceBarData)> {
        self.data
            .range(start_resolution_timestamp..=end_resolution_timestamp)
            .filter_map(|(timestamp, time_price_bar)| {
                time_price_bar.data().map(|data| (timestamp, data))
            })
            .collect()
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

    pub fn update_indicators(&mut self, timestamp: &ResolutionTimestamp) -> Result<()> {
        let indicators = Indicators::compute(timestamp, self.resolution(), self.data());
        self.data
            .get_mut(timestamp)
            .ok_or_else(|| {
                eyre!(
                    "Failed to find Pending TimePriceBar at time {:?}",
                    timestamp
                )
            })
            .and_then(|time_price_bar| match time_price_bar {
                TimePriceBar::Pending(ref mut pending_time_price_bar) => {
                    pending_time_price_bar.set_indicators(indicators);
                    Ok(())
                }
                TimePriceBar::Finalized(_) => Err(eyre!(
                    "Expected Pending TimePriceBar at time {:?}, but found Finalized",
                    timestamp
                )),
            })
    }

    pub fn prune_to_reorged_block_number(
        &mut self,
        reorged_block_number: BlockNumber,
    ) -> Result<()> {
        let mut pruned_timestamp = None;

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
                            // boundary, then we're done in the next iteration
                            time_price_bar.prune_to_reorged_block_number(reorged_block_number);
                            pruned_timestamp = Some(time_price_bar_entry.key().clone());
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

        // If we pruned a time price bar, we have to recompute the indicators
        if let Some(pruned_timestamp) = pruned_timestamp {
            self.update_indicators(&pruned_timestamp)?;
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

        // Insert the data into the time price bar
        match time_price_bar {
            TimePriceBar::Pending(pending_time_price_bar)
                if pending_time_price_bar
                    .end_block_number()
                    .map(|end_block_number| end_block_number + 1 == block_number)
                    .unwrap_or(true) =>
            {
                pending_time_price_bar.insert_block_price_bar(block_number, data);
            }
            TimePriceBar::Pending(_) => {
                return Err(eyre!(
                    "Attempted to insert non-contiguous block number {:?} into Pending TimePriceBar at time {:?}",
                    block_number,
                    block_resolution_timestamp
                ));
            }
            TimePriceBar::Finalized(_) => {
                return Err(eyre!(
                    "Expected Pending TimePriceBar at time {:?} / number {:?}, but found Finalized",
                    block_resolution_timestamp,
                    block_number
                ));
            }
        }

        // Update last inserted timestamp with data
        match self.last_inserted_timestamp_with_data {
            None => self.last_inserted_timestamp_with_data = Some(block_resolution_timestamp),
            Some(timestamp) if timestamp < block_resolution_timestamp => {
                self.last_inserted_timestamp_with_data = Some(block_resolution_timestamp)
            }
            _ => {}
        };

        self.prune_to_retention_count();
        self.update_indicators(&block_resolution_timestamp)?;

        Ok(())
    }

    pub fn finalize_range(
        &mut self,
        start_resolution_timestamp: &ResolutionTimestamp,
        end_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<()> {
        let mut finalized = Vec::new();

        // FIXME: can do this in a range mut
        for (timestamp, price_bar) in self
            .data
            .range(start_resolution_timestamp..=end_resolution_timestamp)
        {
            match price_bar {
                TimePriceBar::Pending(pending_time_price_bar) => {
                    match pending_time_price_bar.as_finalized() {
                        Some(finalized_time_price_bar) => {
                            finalized.push((
                                timestamp.clone(),
                                TimePriceBar::Finalized(finalized_time_price_bar),
                            ));
                        }
                        None => {
                            return Err(eyre!(
                                "Failed to finalize Pending TimePriceBar at {:?}",
                                timestamp
                            ))
                        }
                    }
                }
                TimePriceBar::Finalized(_) => {
                    return Err(eyre!("Expected Pending TimePriceBar, but found Finalized"));
                }
            }
        }

        for (timestamp, finalized_time_price_bar) in finalized {
            self.data.insert(timestamp, finalized_time_price_bar);
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

        match time_price_bar {
            TimePriceBar::Pending(pending_time_price_bar) => {
                pending_time_price_bar
                    .insert_block_price_bar(*block_number, previous_block_price_bar_data);
            }
            TimePriceBar::Finalized(_) => {
                return Err(eyre!(
                    "Expected Pending TimePriceBar at time {:?}, but found Finalized",
                    block_resolution_timestamp
                ));
            }
        }

        self.prune_to_retention_count();
        self.update_indicators(block_resolution_timestamp)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::indexer::{
        time_price_bar_indicators::INDICATOR_BB_PERIOD, Resolution, ResolutionTimestamp,
        TimePriceBarData,
    };

    use super::TimePriceBars;

    use eyre::Result;
    use fraction::GenericFraction;

    #[test]
    pub fn test_insert_data() -> Result<()> {
        let mut time_price_bars = TimePriceBars::new(2, Resolution::FiveMinutes);

        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);

        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );

        // test initial insert
        time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
        let data = time_price_bars.time_price_bar_range(&mock_timestamp, &mock_timestamp);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0], (&mock_timestamp, &mock_data));

        // test prune to retention count
        let next_ts = mock_timestamp.next(&Resolution::FiveMinutes);
        time_price_bars.insert_data(next_ts, 2_u64, mock_data.clone())?;
        let last_ts = next_ts.next(&Resolution::FiveMinutes);
        time_price_bars.insert_data(last_ts, 3_u64, mock_data.clone())?;

        let data = time_price_bars.time_price_bar_range(&mock_timestamp, &last_ts);
        assert_eq!(data.len(), 2);
        assert_eq!(data[0], (&next_ts, &mock_data));

        Ok(())
    }

    #[test]
    pub fn test_prune_to_reorged_block_number() -> Result<()> {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);
        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );

        // test the base case of pending block with partial block data
        {
            let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
            time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
            time_price_bars.insert_data(mock_timestamp, 2_u64, mock_data.clone())?;
            time_price_bars.insert_data(mock_timestamp, 3_u64, mock_data.clone())?;
            time_price_bars.prune_to_reorged_block_number(2_u64)?;
            let data = time_price_bars.time_price_bar_range(&mock_timestamp, &mock_timestamp);
            assert_eq!(data.len(), 1);
        }

        // Alternate case of removing a whole time price bar
        {
            let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
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
            let data = time_price_bars.time_price_bar_range(&mock_timestamp, &mock_timestamp);
            assert_eq!(data.len(), 1);
        }

        Ok(())
    }

    #[test]
    pub fn test_pad_for_block() -> Result<()> {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);

        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );

        // Test padding from pending block
        {
            let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
            time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
            time_price_bars
                .pad_for_block(&2_u64, &mock_timestamp.next(&Resolution::FiveMinutes))?;
            let data = time_price_bars.time_price_bar_range(
                &mock_timestamp,
                &mock_timestamp.next(&Resolution::FiveMinutes),
            );
            assert_eq!(data.len(), 2);
        }

        // Test padding from finalized block
        {
            let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
            time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
            time_price_bars.finalize_range(&mock_timestamp, &mock_timestamp)?;
            time_price_bars
                .pad_for_block(&2_u64, &mock_timestamp.next(&Resolution::FiveMinutes))?;
            let data = time_price_bars.time_price_bar_range(
                &mock_timestamp,
                &mock_timestamp.next(&Resolution::FiveMinutes),
            );
            assert_eq!(data.len(), 2);
        }

        Ok(())
    }

    #[test]
    pub fn test_finalize_range() -> Result<()> {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);
        let mock_data = TimePriceBarData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
        );
        let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
        time_price_bars.insert_data(mock_timestamp, 1_u64, mock_data.clone())?;
        time_price_bars.finalize_range(&mock_timestamp, &mock_timestamp)?;
        let data = time_price_bars.time_price_bar_range(&mock_timestamp, &mock_timestamp);
        assert_eq!(data.len(), 1);

        Ok(())
    }

    #[test]
    pub fn test_update_indicators() -> Result<()> {
        let mut time_price_bars = TimePriceBars::new(100, Resolution::FiveMinutes);

        for i in 1..=INDICATOR_BB_PERIOD {
            time_price_bars.insert_data(
                ResolutionTimestamp::from_timestamp(
                    i * Resolution::FiveMinutes.offset() + 10000,
                    &Resolution::FiveMinutes,
                ),
                i,
                TimePriceBarData::new(
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(i as u128, 1_u128),
                ),
            )?;
        }

        // The last inserted data should contain a set indicator
        let last_inserted_timestamp = ResolutionTimestamp::from_timestamp(
            INDICATOR_BB_PERIOD * Resolution::FiveMinutes.offset() + 10000,
            &Resolution::FiveMinutes,
        );

        assert!(time_price_bars
            .data
            .get(&last_inserted_timestamp)
            .expect(&format!("Expected data for {:?}", last_inserted_timestamp))
            .indicators()
            .is_some());

        Ok(())
    }
}
