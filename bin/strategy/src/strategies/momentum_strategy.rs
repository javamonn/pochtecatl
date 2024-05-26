use super::Strategy;
use pochtecatl_primitives::{Indicators, ResolutionTimestamp, TimePriceBars};

use eyre::{eyre, Result};
use tracing::debug;

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
                        } else if band_mean < ema {
                            Err(eyre!("Band mean {:?} is below EMA {:?}", band_mean, ema))
                        } else if close < band_mean {
                            Err(eyre!(
                                "Close price {:?} is below band mean {:?}",
                                close,
                                band_mean
                            ))
                        } else if ema_slope.is_negative() {
                            Err(eyre!("EMA slope {:?} is negative", ema_slope))
                        } else {
                            debug!(
                                ema = ema.to_string(),
                                band_mean = band_mean.to_string(),
                                close = close.to_string(),
                                ema_slope = ema_slope.to_string(),
                                "should_open_position"
                            );
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
                match time_price_bar.indicators() {
                    Some(Indicators { ema: (ema, _), .. }) => {
                        let close = time_price_bar.close();
                        if close > ema {
                            Err(eyre!("Close {:?} is above EMA {:?}", close, ema))
                        } else {
                            // Close is below EMA after crossing SMA, close the trade
                            debug!(
                                ema = ema.to_string(),
                                close = close.to_string(),
                                "should_close_position"
                            );
                            Ok(())
                        }
                    }
                    None => Err(eyre!("No indicators found for pair.")),
                }
            })
    }
}
