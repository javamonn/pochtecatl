use super::Indicators;
use crate::{Resolution, ResolutionTimestamp, TickData};

use alloy::primitives::{Address, BlockNumber};

use eyre::Result;
use fixed::types::U32F96;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use pochtecatl_db::BacktestTimePriceBarModel;

#[derive(Debug, Serialize, Deserialize)]
pub struct FinalizedTimePriceBar {
    pub data: TickData,
    pub indicators: Option<Indicators>,
    pub start_block_number: BlockNumber,
    pub end_block_number: BlockNumber,
}

impl FinalizedTimePriceBar {
    pub fn new(
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
        data: TickData,
        indicators: Option<Indicators>,
    ) -> Self {
        Self {
            data,
            indicators,
            start_block_number,
            end_block_number,
        }
    }

    pub fn data(&self) -> &TickData {
        &self.data
    }

    pub fn indicators(&self) -> &Option<Indicators> {
        &self.indicators
    }

    pub fn as_backtest_time_price_bar_model(
        &self,
        pair_address: Address,
        resolution: &Resolution,
        resolution_ts: ResolutionTimestamp,
    ) -> Result<BacktestTimePriceBarModel> {
        Ok(BacktestTimePriceBarModel::new(
            pair_address,
            resolution.offset(),
            resolution_ts.0,
            serde_json::to_value(self)?,
        ))
    }
}

// Holds individual BlockPriceBars until the underlying block range has been finalized
#[derive(Debug)]
pub struct PendingTimePriceBar {
    pub data: Option<TickData>,
    pub indicators: Option<Indicators>,
    pub block_price_bars: BTreeMap<BlockNumber, TickData>,
}

impl PendingTimePriceBar {
    pub fn new() -> Self {
        Self {
            block_price_bars: BTreeMap::new(),
            data: None,
            indicators: None,
        }
    }

    pub fn start_block_number(&self) -> Option<&BlockNumber> {
        self.block_price_bars.first_key_value().map(|(k, _)| k)
    }

    pub fn end_block_number(&self) -> Option<&BlockNumber> {
        self.block_price_bars.last_key_value().map(|(k, _)| k)
    }

    pub fn data(&self) -> &Option<TickData> {
        &self.data
    }

    pub fn indicators(&self) -> &Option<Indicators> {
        &self.indicators
    }

    pub fn prune_to_reorged_block_number(&mut self, reorged_block_number: BlockNumber) {
        while let Some(entry) = self.block_price_bars.last_entry() {
            if entry.key() >= &reorged_block_number {
                entry.remove_entry();
            } else {
                break;
            }
        }

        self.data = TickData::reduce(self.block_price_bars.values());
        self.indicators = None;
    }

    pub fn set_indicators(&mut self, indicators: Indicators) {
        self.indicators = Some(indicators);
    }

    pub fn insert_block_price_bar_range<I: Iterator<Item = BlockNumber>>(
        &mut self,
        block_numbers: I,
        data: &TickData,
    ) {
        let mut did_overwrite = false;
        for block_number in block_numbers {
            if self
                .block_price_bars
                .insert(block_number, data.clone())
                .is_some()
            {
                did_overwrite = true;
            }
        }

        match self.data.as_mut() {
            Some(tick) if !did_overwrite => tick.add(&data),
            _ => {
                self.data = TickData::reduce(self.block_price_bars.values());
            }
        };
        self.indicators = None;
    }

    pub fn insert_block_price_bar(&mut self, block_number: BlockNumber, data: TickData) {
        let did_overwrite = self
            .block_price_bars
            .insert(block_number, data.clone())
            .is_some();

        match self.data.as_mut() {
            Some(tick) if !did_overwrite => tick.add(&data),
            _ => {
                self.data = TickData::reduce(self.block_price_bars.values());
            }
        };
        self.indicators = None;
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

        match (start_block_number, end_block_number, self.data.as_ref()) {
            (Some(start_block_number), Some(end_block_number), Some(data)) => {
                Some(FinalizedTimePriceBar::new(
                    start_block_number,
                    end_block_number,
                    data.clone(),
                    self.indicators.clone(),
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
    pub fn data(&self) -> Option<&TickData> {
        match self {
            TimePriceBar::Pending(pending_time_price_bar) => pending_time_price_bar.data().as_ref(),
            TimePriceBar::Finalized(finalized_time_price_bar) => {
                Some(finalized_time_price_bar.data())
            }
        }
    }

    pub fn indicators(&self) -> Option<&Indicators> {
        match self {
            TimePriceBar::Pending(pending_time_price_bar) => {
                pending_time_price_bar.indicators().as_ref()
            }
            TimePriceBar::Finalized(finalized_time_price_bar) => {
                finalized_time_price_bar.indicators().as_ref()
            }
        }
    }

    pub fn close(&self) -> &U32F96 {
        self.data()
            .map(|d| &d.close)
            .unwrap_or_else(|| &U32F96::ZERO)
    }

    pub fn open(&self) -> &U32F96 {
        self.data()
            .map(|d| &d.open)
            .unwrap_or_else(|| &U32F96::ZERO)
    }

    pub fn high(&self) -> &U32F96 {
        self.data()
            .map(|d| &d.high)
            .unwrap_or_else(|| &U32F96::ZERO)
    }

    pub fn low(&self) -> &U32F96 {
        self.data().map(|d| &d.low).unwrap_or_else(|| &U32F96::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::PendingTimePriceBar;
    use crate::{u32f96_from_u256_frac, Indicators, TickData};

    use alloy::primitives::uint;

    use eyre::{Ok, Result};
    use fixed::types::{I32F96, U32F96};

    #[test]
    fn test_pending_time_price_bar_data() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert_block_price_bar(
            1,
            TickData::new(
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                0_u128.into(),
            ),
        );

        assert_eq!(
            time_price_bar
                .data()
                .clone()
                .expect("Expected data but found None"),
            TickData::new(
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                0_u128.into()
            )
        );

        time_price_bar.insert_block_price_bar(
            2,
            TickData::new(
                U32F96::ONE,
                U32F96::from_num(2),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                0_u128.into(),
            ),
        );

        assert_eq!(
            time_price_bar
                .data()
                .clone()
                .expect("Expected data but found None"),
            TickData::new(
                U32F96::ONE,
                U32F96::from_num(2),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                0_u128.into()
            )
        );

        Ok(())
    }

    #[test]
    fn test_block_number() -> Result<()> {
        let mut time_price_bar = PendingTimePriceBar::new();
        time_price_bar.insert_block_price_bar(
            1,
            TickData::new(
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                0_u128.into(),
            ),
        );

        time_price_bar.insert_block_price_bar(
            2,
            TickData::new(
                U32F96::ONE,
                U32F96::from_num(2),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                0_u128.into(),
            ),
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
            TickData::new(
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                0_u128.into(),
            ),
        );
        time_price_bar.insert_block_price_bar(
            2,
            TickData::new(
                U32F96::ONE,
                U32F96::from_num(2),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                0_u128.into(),
            ),
        );
        time_price_bar.insert_block_price_bar(
            3,
            TickData::new(
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                U32F96::from_num(3),
                u32f96_from_u256_frac(uint!(1_U256), uint!(3_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(3_U256)),
                0_u128.into(),
            ),
        );

        assert_eq!(
            time_price_bar
                .data()
                .clone()
                .expect("Expected data but found None"),
            TickData::new(
                U32F96::ONE,
                U32F96::from_num(3),
                u32f96_from_u256_frac(uint!(1_U256), uint!(3_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(3_U256)),
                0_u128.into()
            )
        );

        time_price_bar.prune_to_reorged_block_number(2);

        assert_eq!(
            time_price_bar
                .data()
                .clone()
                .expect("Expected data but found None"),
            TickData::new(
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                0_u128.into()
            )
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
            TickData::new(
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                U32F96::ONE,
                0_u128.into(),
            ),
        );

        time_price_bar.insert_block_price_bar(
            2,
            TickData::new(
                U32F96::ONE,
                U32F96::from_num(2),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                0_u128.into(),
            ),
        );

        time_price_bar.set_indicators(Indicators::new(None, (I32F96::ONE, I32F96::ONE)));

        let finalized = time_price_bar
            .as_finalized()
            .expect("Expected into_finalized,  but found None");

        assert_eq!(finalized.start_block_number, 1);
        assert_eq!(finalized.end_block_number, 2);
        assert_eq!(
            finalized.data,
            TickData::new(
                U32F96::ONE,
                U32F96::from_num(2),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                u32f96_from_u256_frac(uint!(1_U256), uint!(2_U256)),
                0_u128.into()
            )
        );
        assert_eq!(
            finalized.indicators.expect("Expected indicators").ema,
            (I32F96::ONE, I32F96::ONE)
        );
        assert_eq!(
            finalized
                .indicators
                .expect("Expected indicators")
                .bollinger_bands,
            None
        );

        Ok(())
    }
}
