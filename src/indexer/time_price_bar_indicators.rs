use super::{Resolution, ResolutionTimestamp, TimePriceBar};

use fraction::{GenericFraction, Zero};
use lazy_static::lazy_static;
use num_bigint::BigUint;
use std::collections::BTreeMap;
use tracing::warn;

pub const INDICATOR_BB_PERIOD: u64 = 20;
pub const INDICATOR_EMA_SMOOTHING_FACTOR: f64 = 2.0 / (9.0 + 1.0);
pub const SQRT_PRECISION: u32 = 18;

type F = GenericFraction<BigUint>;

lazy_static! {
    pub static ref F_INDICATOR_BB_PERIOD: GenericFraction<BigUint> =
        GenericFraction::new(BigUint::from(INDICATOR_BB_PERIOD), BigUint::from(1_u64));
    pub static ref F_INDICATOR_BB_STD_DEV: GenericFraction<BigUint> =
        GenericFraction::new(BigUint::from(2_u64), BigUint::from(1_u64));
}

#[derive(Debug, Clone)]
pub struct Indicators {
    // sma, upper band, lower band
    pub bollinger_bands: Option<(F, F, F)>,
    // ema, slope
    pub ema: (F, F),
}

impl Indicators {
    fn bollinger_band(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> Option<(F, F, F)> {
        let close_prices = data
            .range(timestamp.decrement(resolution, INDICATOR_BB_PERIOD - 1)..=*timestamp)
            .filter_map(|(_, time_price_bar)| time_price_bar.data().map(|d| d.close.clone()));

        if close_prices.clone().count() != INDICATOR_BB_PERIOD as usize {
            None
        } else {
            let sma = close_prices.clone().sum::<F>() / F_INDICATOR_BB_PERIOD.clone();
            let variance = close_prices
                .map(|price| {
                    let p = price - sma.clone();
                    p.clone() * p
                })
                .sum::<F>()
                / F_INDICATOR_BB_PERIOD.clone();

            let std_dev = variance.sqrt(SQRT_PRECISION);

            Some((
                sma.clone(),
                sma.clone() + (std_dev.clone() * F_INDICATOR_BB_STD_DEV.clone()),
                sma - (std_dev * F_INDICATOR_BB_STD_DEV.clone()),
            ))
        }
    }

    fn ema(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> (F, F) {
        match (
            data.get(timestamp),
            data.get(&timestamp.previous(resolution)),
        ) {
            (Some(time_price_bar), Some(prev_time_price_bar)) => {
                let prev_ema = prev_time_price_bar
                    .indicators()
                    .map(|i| i.ema.0.clone())
                    .unwrap_or_else(|| prev_time_price_bar.close().clone());
                let time_price_bar_close = time_price_bar.close().clone();
                let ema = ((time_price_bar_close - prev_ema.clone())
                    * INDICATOR_EMA_SMOOTHING_FACTOR)
                    + prev_ema.clone();
                (ema.clone(), ema - prev_ema)
            }
            (Some(time_price_bar), None) => {
                (time_price_bar.close().clone(), GenericFraction::zero())
            }
            _ => {
                warn!("No time price bar found at timestamp {:?}", timestamp);
                (GenericFraction::zero(), GenericFraction::zero())
            }
        }
    }

    pub fn new(bollinger_bands: Option<(F, F, F)>, ema: (F, F)) -> Indicators {
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

    use fraction::{GenericDecimal, GenericFraction, One, Zero};
    use num_bigint::BigUint;

    use crate::{
        indexer::{FinalizedTimePriceBar, Resolution, ResolutionTimestamp, TimePriceBar},
        primitives::TickData,
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
                            TickData::new(
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(idx as u128, 1_u128),
                                0_u128.into(),
                            ),
                            Indicators::new(
                                None,
                                (GenericFraction::zero(), GenericFraction::zero()),
                            ),
                        )),
                    );

                    acc
                })
        };

        let (sma, upper_band, lower_band) =
            Indicators::bollinger_band(&mock_timestamp, &Resolution::FiveMinutes, &data)
                .expect("Bollinger band not found");

        assert_eq!(
            format!("{}", GenericDecimal::<BigUint, usize>::from_fraction(sma)),
            "10.5"
        );
        assert_eq!(
            format!(
                "{}",
                GenericDecimal::<BigUint, usize>::from_fraction(upper_band).set_precision(8)
            ),
            "22.03256259"
        );
        assert_eq!(
            format!(
                "{}",
                GenericDecimal::<BigUint, usize>::from_fraction(lower_band).set_precision(8)
            ),
            "-1.03256259"
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
                            TickData::new(
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(1_u128, 1_u128),
                                GenericFraction::new(idx as u128, 1_u128),
                                0_u128.into(),
                            ),
                            Indicators::new(
                                None,
                                (GenericFraction::zero(), GenericFraction::zero()),
                            ),
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
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        0_u128.into(),
                    ),
                    Indicators::new(
                        None,
                        (GenericFraction::new(5_u64, 1_u64), GenericFraction::one()),
                    ),
                )),
            ),
            (
                mock_timestamp,
                TimePriceBar::Finalized(FinalizedTimePriceBar::new(
                    1,
                    1,
                    TickData::new(
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(1_u128, 1_u128),
                        GenericFraction::new(6_u128, 1_u128),
                        0_u128.into(),
                    ),
                    Indicators::new(None, (GenericFraction::zero(), GenericFraction::one())),
                )),
            ),
        ]);

        let (ema, slope) = Indicators::ema(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(
            format!("{}", GenericDecimal::<BigUint, usize>::from_fraction(ema)),
            "5.2"
        );
        assert_eq!(
            format!("{}", GenericDecimal::<BigUint, usize>::from_fraction(slope)),
            "0.2"
        );
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
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(1_u128, 1_u128),
                    GenericFraction::new(6_u128, 1_u128),
                    0_u128.into(),
                ),
                Indicators::new(None, (GenericFraction::zero(), GenericFraction::one())),
            )),
        )]);

        let (ema, slope) = Indicators::ema(&mock_timestamp, &Resolution::FiveMinutes, &data);

        assert_eq!(
            format!("{}", GenericDecimal::<BigUint, usize>::from_fraction(ema)),
            "6"
        );

        assert_eq!(
            format!("{}", GenericDecimal::<BigUint, usize>::from_fraction(slope)),
            "0"
        );
    }
}
