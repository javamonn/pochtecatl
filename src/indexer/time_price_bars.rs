use crate::{config, primitives::TickData};

use super::{Indicators, PendingTimePriceBar, Resolution, ResolutionTimestamp, TimePriceBar};

use alloy::primitives::BlockNumber;

use eyre::{eyre, Result};
use std::collections::BTreeMap;
use tracing::error;

pub struct TimePriceBars {
    data: BTreeMap<ResolutionTimestamp, TimePriceBar>,

    resolution: Resolution,

    // How many historical TimePriceBars to retain. Once exceeded, oldest are
    // pruned first.
    retention_count: u64,

    last_finalized_timestamp: Option<ResolutionTimestamp>,
}

impl TimePriceBars {
    pub fn new(retention_count: u64, resolution: Resolution) -> Self {
        Self {
            data: BTreeMap::new(),
            retention_count,
            resolution,
            last_finalized_timestamp: None,
        }
    }

    pub fn data(&self) -> &BTreeMap<ResolutionTimestamp, TimePriceBar> {
        &self.data
    }

    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }

    #[cfg(test)]
    pub fn last_finalized_timestamp(&self) -> &Option<ResolutionTimestamp> {
        &self.last_finalized_timestamp
    }

    pub fn time_price_bar(&self, timestamp: &ResolutionTimestamp) -> Option<&TimePriceBar> {
        self.data.get(timestamp)
    }

    #[cfg(test)]
    pub fn time_price_bar_range(
        &self,
        start_resolution_timestamp: &ResolutionTimestamp,
        end_resolution_timestamp: &ResolutionTimestamp,
    ) -> Vec<(&ResolutionTimestamp, &TickData)> {
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
        block_number: BlockNumber,
        data: TickData,
        block_timestamp: u64,
        finalized_timestamp: Option<ResolutionTimestamp>,
    ) -> Result<()> {
        let block_resolution_timestamp =
            ResolutionTimestamp::from_timestamp(block_timestamp, &self.resolution);
        let mut updated_block_resolution_timestamps = Vec::new();

        // Pad time price bars with the missing intermediate blocks since the last insert if required
        {
            let pad_from =
                self.data
                    .last_key_value()
                    .and_then(|(_, time_price_bar)| match time_price_bar {
                        TimePriceBar::Pending(price_bar) => price_bar
                            .block_price_bars
                            .last_key_value()
                            .and_then(|(last_block_number, data)| {
                                if last_block_number + 1 != block_number {
                                    Some((*last_block_number, data.clone()))
                                } else {
                                    None
                                }
                            }),
                        TimePriceBar::Finalized(price_bar) => {
                            if price_bar.end_block_number + 1 != block_number {
                                Some((price_bar.end_block_number, price_bar.data.clone()))
                            } else {
                                None
                            }
                        }
                    });

            if let Some((last_inserted_block_number, last_inserted_data)) = pad_from {
                let mut block_numbers_to_pad = Vec::with_capacity(
                    (self.resolution().offset() / *config::AVERAGE_BLOCK_TIME_SECONDS) as usize,
                );
                let mut resolution_timestamp_to_pad = None;
                for padded_block_number in (last_inserted_block_number + 1)..block_number {
                    let padded_resolution_timestamp = ResolutionTimestamp::from_timestamp(
                        block_timestamp
                            - ((block_number - last_inserted_block_number)
                                * *config::AVERAGE_BLOCK_TIME_SECONDS),
                        &self.resolution,
                    );

                    if resolution_timestamp_to_pad
                        .is_some_and(|ts| ts != padded_resolution_timestamp)
                    {
                        let ts = resolution_timestamp_to_pad.unwrap();
                        match self
                            .data
                            .entry(ts)
                            .or_insert_with(|| TimePriceBar::Pending(PendingTimePriceBar::new()))
                        {
                            TimePriceBar::Pending(time_price_bar) => {
                                time_price_bar.insert_block_price_bar_range(
                                    block_numbers_to_pad.drain(..),
                                    &last_inserted_data,
                                );
                                updated_block_resolution_timestamps
                                    .push(padded_resolution_timestamp);
                            }
                            TimePriceBar::Finalized(_) => {
                                error!("Expected Pending TimePriceBar at time {:?}, but found Finalized", ts);
                            }
                        }
                        resolution_timestamp_to_pad = Some(padded_resolution_timestamp);
                    }

                    block_numbers_to_pad.push(padded_block_number);
                    if padded_block_number == block_number - 1 {
                        match self
                            .data
                            .entry(padded_resolution_timestamp)
                            .or_insert_with(|| TimePriceBar::Pending(PendingTimePriceBar::new()))
                        {
                            TimePriceBar::Pending(time_price_bar) => {
                                time_price_bar.insert_block_price_bar_range(
                                    block_numbers_to_pad.drain(..),
                                    &last_inserted_data,
                                );
                                updated_block_resolution_timestamps
                                    .push(padded_resolution_timestamp);
                            }
                            TimePriceBar::Finalized(_) => {
                                error!("Expected Pending TimePriceBar at time {:?}, but found Finalized", padded_resolution_timestamp);
                            }
                        }
                    }
                }
            }
        }

        // Insert the new data into the time price bar
        match self
            .data
            .entry(block_resolution_timestamp.clone())
            .or_insert_with(|| TimePriceBar::Pending(PendingTimePriceBar::new()))
        {
            TimePriceBar::Pending(pending_time_price_bar)
                if pending_time_price_bar
                    .end_block_number()
                    .map(|end_block_number| end_block_number + 1 == block_number)
                    .unwrap_or(true) =>
            {
                pending_time_price_bar.insert_block_price_bar(block_number, data);
                updated_block_resolution_timestamps.push(block_resolution_timestamp);
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
                    block_timestamp,
                    block_resolution_timestamp
                ));
            }
        }

        self.prune_to_retention_count();

        for block_resolution_timestamp in updated_block_resolution_timestamps {
            self.update_indicators(&block_resolution_timestamp)?;
        }

        // Finalize range if required
        match (finalized_timestamp, self.last_finalized_timestamp) {
            (Some(finalized_timestamp), Some(last_finalized_timestamp))
                if finalized_timestamp > last_finalized_timestamp =>
            {
                self.finalize_range(
                    &last_finalized_timestamp.next(&self.resolution),
                    &finalized_timestamp,
                )?;
                self.last_finalized_timestamp = Some(finalized_timestamp);
            }
            (Some(finalized_timestamp), None) => {
                self.finalize_range(&ResolutionTimestamp::zero(), &finalized_timestamp)?;
                self.last_finalized_timestamp = Some(finalized_timestamp);
            }
            _ => {}
        }

        Ok(())
    }

    fn finalize_range(
        &mut self,
        start_resolution_timestamp: &ResolutionTimestamp,
        end_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<()> {
        for (timestamp, price_bar) in self
            .data
            .range_mut(start_resolution_timestamp..=end_resolution_timestamp)
        {
            match price_bar {
                TimePriceBar::Pending(pending_time_price_bar) => {
                    match pending_time_price_bar.as_finalized() {
                        Some(finalized_time_price_bar) => {
                            *price_bar = TimePriceBar::Finalized(finalized_time_price_bar);
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

        Ok(())
    }

    pub fn is_stale(&self, ts: ResolutionTimestamp) -> bool {
        self.data.last_key_value().map_or(true, |(last_ts, _)| {
            (ts.0 - last_ts.0 / self.resolution.offset()) > self.retention_count
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        indexer::{
            time_price_bar_indicators::INDICATOR_BB_PERIOD, Resolution, ResolutionTimestamp,
            TimePriceBar,
        },
        primitives::TickData,
    };

    use super::TimePriceBars;

    use eyre::Result;
    use fraction::GenericFraction;

    #[test]
    pub fn test_insert_data() -> Result<()> {
        let mut time_price_bars = TimePriceBars::new(2, Resolution::FiveMinutes);

        let mock_timestamp = 10000;
        let mock_resolution_timestamp =
            ResolutionTimestamp::from_timestamp(mock_timestamp, &Resolution::FiveMinutes);

        let mock_data = TickData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            0_u128.into(),
        );

        // test initial insert
        time_price_bars.insert_data(1_u64, mock_data.clone(), mock_timestamp, None)?;
        {
            let data = time_price_bars
                .time_price_bar_range(&mock_resolution_timestamp, &mock_resolution_timestamp);
            assert_eq!(data.len(), 1);
            assert_eq!(data[0], (&mock_resolution_timestamp, &mock_data));
        }

        // test prune to retention count
        let next1_ts = mock_resolution_timestamp.next(&Resolution::FiveMinutes);
        let next2_ts = next1_ts.next(&Resolution::FiveMinutes);
        {
            time_price_bars.insert_data(2_u64, mock_data.clone(), next1_ts.0, None)?;
            time_price_bars.insert_data(3_u64, mock_data.clone(), next2_ts.0, None)?;
        }

        assert_eq!(time_price_bars.data.len(), 2);
        {
            let (ts, time_price_bar) = time_price_bars.data.first_key_value().unwrap();
            assert_eq!(ts, &next1_ts);
            assert_eq!(time_price_bar.data().unwrap(), &mock_data);
        }

        // test finalization
        let next3_ts = next2_ts.next(&Resolution::FiveMinutes);
        {
            time_price_bars.insert_data(4_u64, mock_data.clone(), next3_ts.0, Some(next2_ts))?;
        }

        {
            assert!(matches!(
                time_price_bars.data.get(&next2_ts),
                Some(TimePriceBar::Finalized(_))
            ));
            assert!(matches!(
                time_price_bars.data.get(&next3_ts),
                Some(TimePriceBar::Pending(_))
            ));
        }

        Ok(())
    }

    #[test]
    pub fn test_prune_to_reorged_block_number() -> Result<()> {
        let mock_timestamp = 10000;
        let mock_resolution_timestamp =
            ResolutionTimestamp::from_timestamp(mock_timestamp, &Resolution::FiveMinutes);
        let mock_data = TickData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            0_u128.into(),
        );

        // test the base case of pending block with partial block data
        {
            let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
            time_price_bars.insert_data(1_u64, mock_data.clone(), mock_timestamp, None)?;
            time_price_bars.insert_data(2_u64, mock_data.clone(), mock_timestamp, None)?;
            time_price_bars.insert_data(3_u64, mock_data.clone(), mock_timestamp, None)?;
            time_price_bars.prune_to_reorged_block_number(2_u64)?;
            let data = time_price_bars
                .time_price_bar_range(&mock_resolution_timestamp, &mock_resolution_timestamp);
            assert_eq!(data.len(), 1);
        }

        // Alternate case of removing a whole time price bar
        {
            let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
            time_price_bars.insert_data(1_u64, mock_data.clone(), mock_timestamp, None)?;
            time_price_bars.insert_data(
                2_u64,
                mock_data.clone(),
                mock_resolution_timestamp.next(&Resolution::FiveMinutes).0,
                None,
            )?;
            time_price_bars.insert_data(
                3_u64,
                mock_data.clone(),
                mock_resolution_timestamp.next(&Resolution::FiveMinutes).0,
                None,
            )?;
            time_price_bars.prune_to_reorged_block_number(2_u64)?;
            let data = time_price_bars
                .time_price_bar_range(&mock_resolution_timestamp, &mock_resolution_timestamp);
            assert_eq!(data.len(), 1);
        }

        Ok(())
    }

    #[test]
    pub fn test_finalize_range() -> Result<()> {
        let mock_timestamp = 10000;
        let mock_resolution_timestamp =
            ResolutionTimestamp::from_timestamp(mock_timestamp, &Resolution::FiveMinutes);
        let mock_data = TickData::new(
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            GenericFraction::new(1_u128, 1_u128),
            0_u128.into(),
        );
        let mut time_price_bars = TimePriceBars::new(5, Resolution::FiveMinutes);
        time_price_bars.insert_data(1_u64, mock_data.clone(), mock_timestamp, None)?;
        time_price_bars.finalize_range(&mock_resolution_timestamp, &mock_resolution_timestamp)?;
        let data = time_price_bars
            .time_price_bar_range(&mock_resolution_timestamp, &mock_resolution_timestamp);
        assert_eq!(data.len(), 1);

        Ok(())
    }

    #[test]
    pub fn test_update_indicators() -> Result<()> {
        let mut time_price_bars = TimePriceBars::new(100, Resolution::FiveMinutes);

        for i in 1..=INDICATOR_BB_PERIOD {
            time_price_bars.insert_data(
                i,
                TickData::new(
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(i as u128, 1_u128),
                    0_u128.into(),
                ),
                i * Resolution::FiveMinutes.offset() + 10000,
                None,
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
