use std::collections::BTreeMap;

use alloy::primitives::BlockNumber;
use eyre::{eyre, Result};
use fraction::GenericFraction;

use super::{Block, BlockPriceBar};

#[derive(Clone, Copy)]
pub struct TimePriceBarData {
    pub open: GenericFraction<u128>,
    pub high: GenericFraction<u128>,
    pub low: GenericFraction<u128>,
    pub close: GenericFraction<u128>,
}

impl From<BlockPriceBar> for TimePriceBarData {
    fn from(value: BlockPriceBar) -> Self {
        Self {
            open: value.open,
            high: value.high,
            low: value.low,
            close: value.close,
        }
    }
}

pub struct FinalizedTimePriceBar {
    pub data: TimePriceBarData,
    pub start_block_number: BlockNumber,
    pub end_block_number: BlockNumber,
}

impl FinalizedTimePriceBar {
    pub fn new(
        data: TimePriceBarData,
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
    ) -> Self {
        Self {
            data,
            start_block_number,
            end_block_number,
        }
    }
}

// Holds individual BlockPriceBars until the underlying block range has been finalized
pub struct PendingTimePriceBar {
    pub data: Option<TimePriceBarData>,
    pub block_price_bars: BTreeMap<BlockNumber, TimePriceBarData>,
}

impl PendingTimePriceBar {
    pub fn new() -> Self {
        Self {
            block_price_bars: BTreeMap::new(),
            data: None,
        }
    }

    pub fn start_block_number(&self) -> Option<&BlockNumber> {
        self.block_price_bars.first_key_value().map(|(k, v)| k)
    }

    pub fn end_block_number(&self) -> Option<&BlockNumber> {
        self.block_price_bars.last_key_value().map(|(k, v)| k)
    }

    pub fn get_data(&mut self) -> &Option<TimePriceBarData> {
        if self.data.is_none() && self.block_price_bars.len() > 0 {
            let derived_data =
                self.block_price_bars
                    .values()
                    .cloned()
                    .reduce(|mut acc, price_bar| {
                        if price_bar.high > acc.high {
                            acc.high = price_bar.high
                        } else if price_bar.low < acc.low {
                            acc.low = price_bar.low
                        }
                        acc.close = price_bar.close;

                        acc
                    });
            self.data = derived_data
        }

        &self.data
    }

    pub fn prune_to_reorged_block_number(&mut self, reorged_block_number: BlockNumber) {
        while let Some(entry) = self.block_price_bars.last_entry() {
            if entry.key() >= &reorged_block_number {
                entry.remove_entry();
            } else {
                break;
            }
        }
    }

    pub fn insert(&mut self, block_number: BlockNumber, data: TimePriceBarData) -> Result<()> {
        match self.block_price_bars.last_key_value() {
            None => {
                self.block_price_bars.insert(block_number, data);
                Ok(())
            }
            Some((last_block_number, _)) => {
                if last_block_number + 1 == block_number {
                    self.block_price_bars.insert(block_number, data);
                    Ok(())
                } else {
                    Err(eyre!("block_price_bars should be contiguous: last_block_number {}, inserting block number {}", last_block_number, block_number))
                }
            }
        }
    }

    pub fn into_finalized(&mut self) -> Option<FinalizedTimePriceBar> {
        let start_block_number = self
            .block_price_bars
            .first_key_value()
            .map(|(start_block_number, _)| start_block_number.clone());
        let end_block_number = self
            .block_price_bars
            .last_key_value()
            .map(|(end_block_number, _)| end_block_number.clone());
        match (self.get_data(), start_block_number, end_block_number) {
            (Some(data), Some(start_block_number), Some(end_block_number)) => {
                Some(FinalizedTimePriceBar::new(
                    data.clone(),
                    start_block_number.clone(),
                    end_block_number.clone(),
                ))
            }
            _ => None,
        }
    }
}

pub enum TimePriceBar {
    Pending(PendingTimePriceBar),
    Finalized(FinalizedTimePriceBar),
}

impl TimePriceBar {
    pub fn insert(&mut self, block_number: BlockNumber, data: TimePriceBarData) -> Result<()> {
        match self {
            TimePriceBar::Pending(pending_time_price_bar) => {
                pending_time_price_bar.insert(block_number, data)
            }
            TimePriceBar::Finalized(_) => Err(eyre!(
                "Attempted to insert price bar data into finalized price bar for block number {}",
                block_number
            )),
        }
    }

    pub fn into_finalized(&mut self) -> Result<Self> {
        match self {
            TimePriceBar::Pending(price_bar) => price_bar
                .into_finalized()
                .ok_or_else(|| eyre!("Unable to finalize price bar"))
                .map(|price_bar| TimePriceBar::Finalized(price_bar)),
            TimePriceBar::Finalized(_) => Err(eyre!("Price bar already finalized")),
        }
    }
}
