use super::{Resolution, ResolutionTimestamp, TimePriceBar};

use fixed::{
    types::{I32F96, U32F96},
    FixedU128,
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ops::Bound};
use tracing::warn;

pub const INDICATOR_BB_PERIOD: u64 = 20;
pub const INDICATOR_EMA_PERIOD: u64 = 9;

lazy_static! {
    pub static ref Q_INDICATOR_BB_PERIOD: U32F96 = FixedU128::from_num(INDICATOR_BB_PERIOD);
    pub static ref Q_INDICATOR_BB_STD_DEV: U32F96 = FixedU128::from_num(2.0);
    pub static ref Q_INDICATOR_EMA_SMOOTHING_FACTOR: I32F96 =
        I32F96::from_num(2) / (I32F96::from_num(INDICATOR_EMA_PERIOD) + I32F96::ONE);
}

#[derive(Debug, Clone, Copy)]
pub enum IndicatorsConfig {
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Indicators {
    // sma, upper band, lower band, sma slope
    pub bollinger_bands: Option<(U32F96, U32F96, U32F96, I32F96)>,
    // ema, slope
    pub ema: (I32F96, I32F96),
}

impl Indicators {
    fn bollinger_band(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> Option<(U32F96, U32F96, U32F96, I32F96)> {
        let close_prices = data
            .range((Bound::Unbounded, Bound::Included(timestamp)))
            .rev()
            .take(INDICATOR_BB_PERIOD as usize)
            .filter_map(|(_, time_price_bar)| time_price_bar.data().map(|d| d.close.clone()));

        if close_prices.clone().count() != INDICATOR_BB_PERIOD as usize {
            None
        } else {
            let sma = close_prices.clone().sum::<U32F96>() / *Q_INDICATOR_BB_PERIOD;
            let variance = {
                let variance_sma = sma.to_num::<I32F96>();
                close_prices
                    .map(|price| {
                        let p = price.to_num::<I32F96>() - variance_sma;
                        (p * p).to_num::<U32F96>()
                    })
                    .sum::<U32F96>()
                    / *Q_INDICATOR_BB_PERIOD
            };
            let std_dev = variance.sqrt();
            let sma_slope = data
                .get(&timestamp.previous(resolution))
                .and_then(|t| t.indicators())
                .and_then(|i| i.bollinger_bands)
                .map(|(prev_sma, _, _, _)| sma.to_num::<I32F96>() - prev_sma.to_num::<I32F96>())
                .unwrap_or(I32F96::ZERO);

            Some((
                sma.clone(),
                sma.clone() + (std_dev * *Q_INDICATOR_BB_STD_DEV),
                sma.checked_sub(std_dev * *Q_INDICATOR_BB_STD_DEV)
                    .unwrap_or(U32F96::ZERO),
                sma_slope,
            ))
        }
    }

    fn ema(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> (I32F96, I32F96) {
        match (
            data.get(timestamp),
            data.get(&timestamp.previous(resolution)),
        ) {
            (Some(time_price_bar), Some(prev_time_price_bar)) => {
                let prev_ema = prev_time_price_bar
                    .indicators()
                    .map(|i| i.ema.0)
                    .unwrap_or_else(|| prev_time_price_bar.close().to_num::<I32F96>());
                let ema = (time_price_bar.close().to_num::<I32F96>() - prev_ema)
                    * *Q_INDICATOR_EMA_SMOOTHING_FACTOR
                    + prev_ema;
                (ema, ema - prev_ema)
            }
            (Some(time_price_bar), None) => {
                (time_price_bar.close().to_num::<I32F96>(), I32F96::ZERO)
            }
            _ => {
                warn!("No time price bar found at timestamp {:?}", timestamp);
                (I32F96::ZERO, I32F96::ZERO)
            }
        }
    }

    pub fn new(
        bollinger_bands: Option<(U32F96, U32F96, U32F96, I32F96)>,
        ema: (I32F96, I32F96),
    ) -> Indicators {
        Indicators {
            bollinger_bands,
            ema,
        }
    }

    pub fn compute(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
        _config: &IndicatorsConfig,
    ) -> Indicators {
        let bollinger_bands = Indicators::bollinger_band(timestamp, resolution, data);
        let ema = Indicators::ema(timestamp, resolution, data);

        Indicators::new(bollinger_bands, ema)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        u32f96_from_u256_frac, FinalizedTimePriceBar, Resolution, ResolutionTimestamp, TickData,
        TimePriceBar,
    };

    use super::{Indicators, INDICATOR_BB_PERIOD};

    use alloy::primitives::{uint, U256};

    use fixed::types::{I32F96, U32F96};
    use std::collections::BTreeMap;

    #[test]
    fn test_bollinger_band() {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);
        let data = {
            let start = mock_timestamp
                .decrement(&Resolution::FiveMinutes, INDICATOR_BB_PERIOD)
                .0 as u64;
            let end = mock_timestamp.0 as u64;

            (start..=end)
                .step_by(Resolution::FiveMinutes.offset() as usize)
                .enumerate()
                .fold(BTreeMap::new(), |mut acc, (idx, ts)| {
                    acc.insert(
                        ResolutionTimestamp::from_timestamp(ts, &Resolution::FiveMinutes),
                        TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                            1,
                            1,
                            TickData::new(
                                U32F96::ONE,
                                U32F96::ONE,
                                U32F96::ONE,
                                u32f96_from_u256_frac(U256::from(idx), uint!(1_U256)),
                                0_u128.into(),
                            ),
                            Some(Indicators::new(None, (I32F96::ZERO, I32F96::ZERO))),
                        )),
                    );

                    acc
                })
        };

        let (sma, upper_band, lower_band, _) =
            Indicators::bollinger_band(&mock_timestamp, &Resolution::FiveMinutes, &data)
                .expect("Bollinger band not found");

        assert_eq!(sma.to_string(), "10.5");
        assert_eq!(upper_band.to_string(), "22.0325625946707958893541832388");
        assert_eq!(lower_band.to_string(), "0");
    }

    #[test]
    fn test_empty_bollinger_band() {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);
        let data = {
            let start = mock_timestamp
                .decrement(&Resolution::FiveMinutes, INDICATOR_BB_PERIOD - 2)
                .0 as u64;
            let end = mock_timestamp.0 as u64;

            (start..=end)
                .step_by(Resolution::FiveMinutes.offset() as usize)
                .enumerate()
                .fold(BTreeMap::new(), |mut acc, (idx, ts)| {
                    acc.insert(
                        ResolutionTimestamp::from_timestamp(ts, &Resolution::FiveMinutes),
                        TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                            1,
                            1,
                            TickData::new(
                                U32F96::ONE,
                                U32F96::ONE,
                                U32F96::ONE,
                                u32f96_from_u256_frac(U256::from(idx), uint!(1_U256)),
                                0_u128.into(),
                            ),
                            Some(Indicators::new(None, (I32F96::ZERO, I32F96::ZERO))),
                        )),
                    );

                    acc
                })
        };

        let result = Indicators::bollinger_band(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(result, None);
    }

    #[test]
    fn test_ema() {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);
        let data = BTreeMap::from([
            (
                mock_timestamp.previous(&Resolution::FiveMinutes),
                TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                    1,
                    1,
                    TickData::new(
                        U32F96::ONE,
                        U32F96::ONE,
                        U32F96::ONE,
                        U32F96::ONE,
                        0_u128.into(),
                    ),
                    Some(Indicators::new(None, (I32F96::from_num(5), I32F96::ONE))),
                )),
            ),
            (
                mock_timestamp,
                TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                    1,
                    1,
                    TickData::new(
                        U32F96::ONE,
                        U32F96::ONE,
                        U32F96::ONE,
                        U32F96::from_num(6),
                        0_u128.into(),
                    ),
                    Some(Indicators::new(None, (I32F96::ZERO, I32F96::ONE))),
                )),
            ),
        ]);

        let (ema, slope) = Indicators::ema(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(ema.to_string(), "5.2");
        assert_eq!(slope.to_string(), "0.2");
    }

    #[test]
    fn test_first_ema() {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);
        let data = BTreeMap::from([(
            mock_timestamp,
            TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                1,
                1,
                TickData::new(
                    U32F96::ONE,
                    U32F96::ONE,
                    U32F96::ONE,
                    U32F96::from_num(6),
                    0_u128.into(),
                ),
                Some(Indicators::new(None, (I32F96::ZERO, I32F96::ONE))),
            )),
        )]);

        let (ema, slope) = Indicators::ema(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(ema.to_string(), "6");
        assert_eq!(slope.to_string(), "0");
    }
}
