use super::{BlockPriceBar, Indicators};

use alloy::primitives::BlockNumber;

use fraction::{GenericFraction, ToPrimitive};
use std::collections::BTreeMap;

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

    pub fn reduce<'a>(
        data: impl Iterator<Item = &'a TimePriceBarData>,
    ) -> Option<TimePriceBarData> {
        data.fold(None, |acc, price_bar| match acc {
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

    pub fn is_negative(&self) -> bool {
        self.close < self.open
    }

    pub fn close(&self) -> f64 {
        self.close.to_f64().unwrap_or_else(|| {
            log::warn!(
                "Unable to derive close price for time price bar: {:?}",
                self
            );
            f64::NAN
        })
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

#[derive(Debug)]
pub struct FinalizedTimePriceBar {
    pub data: TimePriceBarData,
    pub indicators: Indicators,
    pub close: f64,
    pub start_block_number: BlockNumber,
    pub end_block_number: BlockNumber,
}

impl FinalizedTimePriceBar {
    pub fn new(
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
        data: TimePriceBarData,
        indicators: Indicators,
    ) -> Self {
        Self {
            data,
            indicators,
            start_block_number,
            end_block_number,
            close: data.close(),
        }
    }

    pub fn data(&self) -> &TimePriceBarData {
        &self.data
    }

    pub fn indicators(&self) -> &Indicators {
        &self.indicators
    }

    pub fn close(&self) -> f64 {
        self.close
    }
}

// Holds individual BlockPriceBars until the underlying block range has been finalized
#[derive(Debug)]
pub struct PendingTimePriceBar {
    pub data: Option<TimePriceBarData>,
    pub indicators: Option<Indicators>,
    pub close: Option<f64>,
    pub block_price_bars: BTreeMap<BlockNumber, TimePriceBarData>,
}

impl PendingTimePriceBar {
    pub fn new() -> Self {
        Self {
            block_price_bars: BTreeMap::new(),
            data: None,
            close: None,
            indicators: None,
        }
    }

    pub fn start_block_number(&self) -> Option<&BlockNumber> {
        self.block_price_bars.first_key_value().map(|(k, _)| k)
    }

    pub fn end_block_number(&self) -> Option<&BlockNumber> {
        self.block_price_bars.last_key_value().map(|(k, _)| k)
    }

    pub fn data(&self) -> &Option<TimePriceBarData> {
        &self.data
    }

    pub fn indicators(&self) -> &Option<Indicators> {
        &self.indicators
    }

    pub fn close(&self) -> f64 {
        self.close.unwrap_or_else(|| {
            log::warn!(
                "Unable to derive close price for time price bar: {:?}",
                self
            );
            f64::NAN
        })
    }

    pub fn prune_to_reorged_block_number(&mut self, reorged_block_number: BlockNumber) {
        while let Some(entry) = self.block_price_bars.last_entry() {
            if entry.key() >= &reorged_block_number {
                entry.remove_entry();
            } else {
                break;
            }
        }

        self.data = TimePriceBarData::reduce(self.block_price_bars.values());
        self.close = self.data.as_ref().map(|data| data.close());
        self.indicators = None;
    }

    pub fn set_indicators(&mut self, indicators: Indicators) {
        self.indicators = Some(indicators);
    }

    pub fn insert_block_price_bar(&mut self, block_number: BlockNumber, data: TimePriceBarData) {
        self.block_price_bars.insert(block_number, data);
        self.data = TimePriceBarData::reduce(self.block_price_bars.values());
        self.indicators = None;
        self.close = self.data.as_ref().map(|data| data.close());
    }

    pub fn as_finalized(&self) -> Option<FinalizedTimePriceBar> {
        let start_block_number = self
            .block_price_bars
            .first_key_value()
            .map(|(start_block_number, _)| start_block_number.clone());
        let end_block_number = self
            .block_price_bars
            .last_key_value()
            .map(|(end_block_number, _)| end_block_number.clone());

        match (
            start_block_number,
            end_block_number,
            self.data.as_ref(),
            self.indicators.as_ref(),
        ) {
            (Some(start_block_number), Some(end_block_number), Some(data), Some(indicators)) => {
                Some(FinalizedTimePriceBar::new(
                    start_block_number,
                    end_block_number,
                    data.clone(),
                    indicators.clone(),
                ))
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum TimePriceBar {
    Pending(PendingTimePriceBar),
    Finalized(FinalizedTimePriceBar),
}

impl TimePriceBar {
    pub fn data(&self) -> Option<&TimePriceBarData> {
        match self {
            TimePriceBar::Pending(pending_time_price_bar) => pending_time_price_bar.data().as_ref(),
            TimePriceBar::Finalized(finalized_time_price_bar) => {
                Some(finalized_time_price_bar.data())
            }
        }
    }

    pub fn close(&self) -> f64 {
        match self {
            TimePriceBar::Pending(pending_time_price_bar) => pending_time_price_bar.close(),
            TimePriceBar::Finalized(finalized_time_price_bar) => finalized_time_price_bar.close(),
        }
    }

    pub fn indicators(&self) -> Option<&Indicators> {
        match self {
            TimePriceBar::Pending(pending_time_price_bar) => {
                pending_time_price_bar.indicators().as_ref()
            }
            TimePriceBar::Finalized(finalized_time_price_bar) => {
                Some(finalized_time_price_bar.indicators())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::indexer::{Indicators, PendingTimePriceBar, TimePriceBarData};

    use eyre::{Ok, Result};
    use fraction::GenericFraction;

    #[test]
    fn test_pending_time_price_bar_data() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert_block_price_bar(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        );

        assert_eq!(
            time_price_bar.data().expect("Expected data but found None"),
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128)
            }
        );

        time_price_bar.insert_block_price_bar(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
        );

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
        time_price_bar.insert_block_price_bar(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        );

        time_price_bar.insert_block_price_bar(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
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
            &2_u64
        );

        Ok(())
    }

    #[test]
    fn test_prune_to_reorged_block_number() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert_block_price_bar(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        );
        time_price_bar.insert_block_price_bar(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
        );
        time_price_bar.insert_block_price_bar(
            3,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 2_u128),
                high: GenericFraction::new(3_u128, 1_u128),
                low: GenericFraction::new(1_u128, 3_u128),
                close: GenericFraction::new(1_u128, 3_u128),
            },
        );

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
        time_price_bar.insert_block_price_bar(
            1,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(1_u128, 1_u128),
                low: GenericFraction::new(1_u128, 1_u128),
                close: GenericFraction::new(1_u128, 1_u128),
            },
        );

        time_price_bar.insert_block_price_bar(
            2,
            TimePriceBarData {
                open: GenericFraction::new(1_u128, 1_u128),
                high: GenericFraction::new(2_u128, 1_u128),
                low: GenericFraction::new(1_u128, 2_u128),
                close: GenericFraction::new(1_u128, 2_u128),
            },
        );

        time_price_bar.set_indicators(Indicators::new(None, (1.0, 1.0)));

        let finalized = time_price_bar
            .as_finalized()
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
        assert_eq!(finalized.indicators.ema, (1.0, 1.0));
        assert_eq!(finalized.indicators.bollinger_bands, None);

        Ok(())
    }
}
