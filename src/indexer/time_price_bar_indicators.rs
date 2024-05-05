use super::{Resolution, ResolutionTimestamp, TimePriceBar};

use std::collections::BTreeMap;
use tracing::warn;

pub const INDICATOR_EMA_SMOOTHING_FACTOR: f64 = 2.0 / (9.0 + 1.0);
pub const INDICATOR_BB_PERIOD: u64 = 20;
pub const INDICATOR_BB_STD_DEV: f64 = 2.0;

#[derive(Debug, Clone, Copy)]
pub struct Indicators {
    // sma, upper band, lower band
    pub bollinger_bands: Option<(f64, f64, f64)>,
    // ema, slope
    pub ema: (f64, f64),
}

impl Indicators {
    fn bollinger_band(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> Option<(f64, f64, f64)> {
        let close_prices = data
            .range(timestamp.decrement(resolution, INDICATOR_BB_PERIOD - 1)..=*timestamp)
            .map(|(_, time_price_bar)| time_price_bar.close());

        if close_prices.clone().count() != INDICATOR_BB_PERIOD as usize {
            None
        } else {
            let sma = close_prices.clone().sum::<f64>() / INDICATOR_BB_PERIOD as f64;
            let variance = close_prices.map(|price| (price - sma).powi(2)).sum::<f64>()
                / INDICATOR_BB_PERIOD as f64;
            let std_dev = variance.sqrt();

            Some((
                sma,
                sma + (std_dev * INDICATOR_BB_STD_DEV),
                sma - (std_dev * INDICATOR_BB_STD_DEV),
            ))
        }
    }

    fn ema(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> (f64, f64) {
        match (
            data.get(timestamp),
            data.get(&timestamp.previous(resolution)),
        ) {
            (Some(time_price_bar), Some(prev_time_price_bar)) => {
                let prev_ema = prev_time_price_bar
                    .indicators()
                    .map(|i| i.ema.0)
                    .unwrap_or(prev_time_price_bar.close());
                let ema = ((time_price_bar.close() - prev_ema) * INDICATOR_EMA_SMOOTHING_FACTOR)
                    + prev_ema;
                (ema, ema - prev_ema)
            }
            (Some(time_price_bar), None) => (time_price_bar.close(), 0.0),
            _ => {
                warn!("No time price bar found at timestamp {:?}", timestamp);
                (f64::NAN, 0.0)
            }
        }
    }

    pub fn new(bollinger_bands: Option<(f64, f64, f64)>, ema: (f64, f64)) -> Indicators {
        Indicators {
            bollinger_bands,
            ema,
        }
    }

    pub fn compute(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> Indicators {
        let bollinger_bands = Indicators::bollinger_band(timestamp, resolution, data);
        let ema = Indicators::ema(timestamp, resolution, data);

        Indicators::new(bollinger_bands, ema)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fraction::GenericFraction;

    use crate::indexer::{
        FinalizedTimePriceBar, Resolution, ResolutionTimestamp, TimePriceBar, TimePriceBarData,
    };

    use super::{Indicators, INDICATOR_BB_PERIOD};

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
                            TimePriceBarData::new(
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(idx as u128, 1_u128),
                            ),
                            Indicators::new(None, (0.0, 0.0)),
                        )),
                    );

                    acc
                })
        };

        let result = Indicators::bollinger_band(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(
            result,
            Some((10.5, 22.032562594670797, -1.0325625946707966))
        );
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
                            TimePriceBarData::new(
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(idx as u128, 1_u128),
                            ),
                            Indicators::new(None, (0.0, 0.0)),
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
                    TimePriceBarData::new(
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                    ),
                    Indicators::new(None, (5.0, 1.0)),
                )),
            ),
            (
                mock_timestamp,
                TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                    1,
                    1,
                    TimePriceBarData::new(
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(6_u128, 1_u128),
                    ),
                    Indicators::new(None, (0.0, 1.0)),
                )),
            ),
        ]);

        let result = Indicators::ema(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(result, (5.2, 0.20000000000000018));
    }

    #[test]
    fn test_first_ema() {
        let mock_timestamp = ResolutionTimestamp::from_timestamp(10000, &Resolution::FiveMinutes);
        let data = BTreeMap::from([(
            mock_timestamp,
            TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                1,
                1,
                TimePriceBarData::new(
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(6_u128, 1_u128),
                ),
                Indicators::new(None, (0.0, 1.0)),
            )),
        )]);

        let result = Indicators::ema(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(result, (6.0, 0.0));
    }
}
