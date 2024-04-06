use std::collections::BTreeMap;

use alloy::primitives::BlockNumber;
use eyre::{eyre, Result};
use fraction::GenericFraction;

use super::BlockPriceBar;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimePriceBarData {
    pub open: GenericFraction<u128>,
    pub high: GenericFraction<u128>,
    pub low: GenericFraction<u128>,
    pub close: GenericFraction<u128>,
}

impl TimePriceBarData {
    #[cfg(test)]
    pub fn new(
        open: GenericFraction<u128>,
        high: GenericFraction<u128>,
        low: GenericFraction<u128>,
        close: GenericFraction<u128>,
    ) -> Self {
        Self {
            open,
            high,
            low,
            close,
        }
    }
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

#[derive(Debug, PartialEq, Eq)]
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
#[derive(Debug, PartialEq, Eq)]
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
        self.block_price_bars.first_key_value().map(|(k, _)| k)
    }

    pub fn end_block_number(&self) -> Option<&BlockNumber> {
        self.block_price_bars.last_key_value().map(|(k, _)| k)
    }

    fn derive_data(&self) -> Option<TimePriceBarData> {
        self.block_price_bars
            .values()
            .fold(None, |acc, price_bar| match acc {
                None => Some(price_bar.clone()),
                Some(mut acc) => {
                    if price_bar.high > acc.high {
                        acc.high = price_bar.high
                    }
                    if price_bar.low < acc.low {
                        acc.low = price_bar.low
                    }
                    acc.close = price_bar.close;

                    Some(acc)
                }
            })
    }

    pub fn data(&self) -> &Option<TimePriceBarData> {
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

        self.data = self.derive_data();
    }

    pub fn insert(&mut self, block_number: BlockNumber, data: TimePriceBarData) -> Result<()> {
        match self.block_price_bars.last_key_value() {
            None => {
                self.block_price_bars.insert(block_number, data);
                self.data = self.derive_data();
                Ok(())
            }
            Some((last_block_number, _)) => {
                if last_block_number + 1 == block_number {
                    self.block_price_bars.insert(block_number, data);
                    self.data = self.derive_data();
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

        if self.data.is_none() {
            self.data = self.derive_data();
        }

        match (self.data(), start_block_number, end_block_number) {
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

#[derive(Debug, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::{PendingTimePriceBar, TimePriceBarData};

    use eyre::{Ok, Result};
    use fraction::GenericFraction;

    #[test]
    fn test_pending_time_price_bar_data() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        )?;

        assert_eq!(
            time_price_bar.data().expect("Expected data but found None"),
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128)
            }
        );

        time_price_bar.insert(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
        )?;

        assert_eq!(
            time_price_bar.data().expect("Expected data but found None"),
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128)
            }
        );

        Ok(())
    }

    #[test]
    fn test_block_number() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        )?;

        time_price_bar.insert(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
        )?;

        assert_eq!(
            time_price_bar
                .start_block_number()
                .expect("Expected start_block_number, but found None"),
            &1_u64
        );
        assert_eq!(
            time_price_bar
                .end_block_number()
                .expect("Expected start_block_number, but found None"),
            &2_u64
        );

        Ok(())
    }

    #[test]
    fn test_prune_to_reorged_block_number() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        )?;
        time_price_bar.insert(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
        )?;
        time_price_bar.insert(
            3,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 2_u128),
                high: GenericFraction::new(3_u128, 1_u128),
                low: GenericFraction::new(1_u128, 3_u128),
                close: GenericFraction::new(1_u128, 3_u128),
            },
        )?;

        assert_eq!(
            time_price_bar.data().expect("Expected data but found None"),
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(3_u128, 1_u128),
                low: GenericFraction::new(1_u128, 3_u128),
                close: GenericFraction::new(1_u128, 3_u128)
            }
        );

        time_price_bar.prune_to_reorged_block_number(2);

        assert_eq!(
            time_price_bar.data().expect("Expected data but found None"),
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128)
            }
        );
        assert_eq!(
            time_price_bar
                .start_block_number()
                .expect("Expected start_block_number, but found None"),
            &1_u64
        );
        assert_eq!(
            time_price_bar
                .end_block_number()
                .expect("Expected start_block_number, but found None"),
            &1_u64
        );

        Ok(())
    }

    #[test]
    fn test_into_finalized() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        )?;

        time_price_bar.insert(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
        )?;

        let finalized = time_price_bar
            .into_finalized()
            .expect("Expected into_finalized,  but found None");

        assert_eq!(finalized.start_block_number, 1);
        assert_eq!(finalized.end_block_number, 2);
        assert_eq!(
            finalized.data,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128)
            }
        );

        Ok(())
    }
}
