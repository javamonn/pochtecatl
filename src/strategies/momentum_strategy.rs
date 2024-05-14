use super::Strategy;
use crate::indexer::{Indicators, ResolutionTimestamp, TimePriceBars};

use eyre::{eyre, Result};

pub struct MomentumStrategy {}

impl MomentumStrategy {
    pub fn new() -> Self {
        Self {}
    }
}

impl Strategy for MomentumStrategy {
    fn should_open_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        timestamp: &ResolutionTimestamp,
    ) -> Result<()> {
        pair_time_price_bars
            .time_price_bar(timestamp)
            .ok_or_else(|| eyre!("No time price bar found for pair",))
            .and_then(|time_price_bar| {
                // Ensure the most recent time price bar is positive
                if time_price_bar
                    .data()
                    .map(|d| d.is_negative())
                    .unwrap_or(true)
                {
                    return Err(eyre!("Time price bar is negative"));
                }

                match time_price_bar.indicators() {
                    Some(Indicators {
                        ema: (ema, ema_slope),
                        bollinger_bands: Some((band_mean, _, _)),
                    }) => {
                        let close = time_price_bar.close();
                        if close < ema {
                            Err(eyre!("Close price {:?} is below EMA {:?}", close, ema))
                        } else if close > band_mean {
                            Err(eyre!(
                                "Close price {:?} is above bollinger band mean {:?}",
                                close,
                                band_mean
                            ))
                        } else if ema_slope.is_sign_negative() {
                            Err(eyre!("EMA slope {:?} is negative", ema_slope))
                        } else {
                            // open a trade
                            Ok(())
                        }
                    }
                    Some(_) => Err(eyre!("No bollinger bands found for pair.")),
                    None => Err(eyre!("No indicators found for pair.",)),
                }
            })
    }

    fn should_close_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        now_block_resolution_timestamp: &ResolutionTimestamp,
        open_block_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<()> {
        pair_time_price_bars
            .time_price_bar(now_block_resolution_timestamp)
            .ok_or_else(|| eyre!("No time price bar found for pair",))
            .and_then(|time_price_bar| {
                // If we crossed SMA since entry, close price should be above SMA.
                // Otherwise entry on EMA cross, so verify that we're still above that.
                let has_crossed_sma = pair_time_price_bars
                    .data()
                    .range(open_block_resolution_timestamp..=now_block_resolution_timestamp)
                    .any(|(_, time_price_bar)| match time_price_bar.indicators() {
                        Some(Indicators {
                            bollinger_bands: Some((sma, _, _)),
                            ..
                        }) => time_price_bar.close() >= sma,
                        _ => false,
                    });

                match time_price_bar.indicators() {
                    Some(Indicators { ema: (ema, _), .. }) if has_crossed_sma => {
                        if time_price_bar.close() > ema {
                            Err(eyre!("Close is above EMA after crossing SMA"))
                        } else {
                            // Close is below EMA after crossing SMA, close the trade
                            Ok(())
                        }
                    }
                    Some(Indicators {
                        bollinger_bands: Some((sma, _, _)),
                        ..
                    }) => {
                        if time_price_bar.close() > sma {
                            Err(eyre!("Close is above SMA after entry"))
                        } else {
                            // Close is below SMA after entry, close the trade
                            Ok(())
                        }
                    }
                    Some(_) => Err(eyre!("No bollinger bands found for pair.")),
                    None => Err(eyre!("No indicators found for pair.")),
                }
            })
    }
}
