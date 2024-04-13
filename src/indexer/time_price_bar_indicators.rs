use super::{Resolution, ResolutionTimestamp, TimePriceBar};

use std::collections::BTreeMap;

pub const INDICATOR_EMA_SMOOTHING_FACTOR: f64 = 2.0 / (9.0 + 1.0);
pub const INDICATOR_BB_PERIOD: u64 = 20;
pub const INDICATOR_BB_STD_DEV: f64 = 2.0;

#[derive(Debug, Clone, Copy)]
pub struct Indicators {
    pub bollinger_bands: (f64, f64, f64),
    pub ema: f64,
}

impl Indicators {
    fn bollinger_band(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> (f64, f64, f64) {
        let close_prices = data
            .range(timestamp.decrement(resolution, INDICATOR_BB_PERIOD)..=*timestamp)
            .map(|(_, time_price_bar)| time_price_bar.close());

        let sma = close_prices.clone().sum::<f64>() / INDICATOR_BB_PERIOD as f64;
        let variance = close_prices.map(|price| (price - sma).powi(2)).sum::<f64>()
            / INDICATOR_BB_PERIOD as f64;
        let std_dev = variance.sqrt();

        (
            sma,
            sma + (std_dev * INDICATOR_BB_STD_DEV),
            sma - (std_dev * INDICATOR_BB_STD_DEV),
        )
    }

    fn ema(
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        data: &BTreeMap<ResolutionTimestamp, TimePriceBar>,
    ) -> f64 {
        match (
            data.get(timestamp),
            data.get(&timestamp.previous(resolution)),
        ) {
            (Some(time_price_bar), Some(prev_time_price_bar)) => {
                let prev_ema = prev_time_price_bar
                    .indicators()
                    .map(|i| i.ema)
                    .unwrap_or(prev_time_price_bar.close());
                ((time_price_bar.close() - prev_ema) * INDICATOR_EMA_SMOOTHING_FACTOR) + prev_ema
            }
            (Some(time_price_bar), None) => time_price_bar.close(),
            _ => {
                log::warn!("No time price bar found at timestamp {:?}", timestamp);
                f64::NAN
            }
        }
    }

    pub fn new(bollinger_bands: (f64, f64, f64), ema: f64) -> Indicators {
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
