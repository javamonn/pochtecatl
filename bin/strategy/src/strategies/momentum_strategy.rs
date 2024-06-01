use super::Strategy;
use pochtecatl_primitives::{
    Indicators, Resolution, ResolutionTimestamp, TimePriceBars, TradeMetadata,
};

use eyre::{eyre, Result};
use tracing::debug;

pub struct MomentumStrategy {}

impl MomentumStrategy {
    pub fn new() -> Self {
        Self {}
    }
}

impl Strategy for MomentumStrategy {
    // TODO:
    // - do not open if we've had another trade since the last ema over sma cross
    // - do not open if we've had another trade in this time price bar
    fn should_open_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        previous_close_trade_metadata: Option<&TradeMetadata>,
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

                let previous_trade_resolution_ts =
                    previous_close_trade_metadata.map(|previous_close_trade| {
                        ResolutionTimestamp::from_timestamp(
                            *previous_close_trade.block_timestamp(),
                            &resolution,
                        )
                    });

                if previous_trade_resolution_ts.is_some_and(|previous_trade_resolution_ts| {
                    previous_trade_resolution_ts == *timestamp
                }) {
                    // Ensure we did not open a position on the same time price bar
                    return Err(eyre!("Previous trade closed on the same time price bar"));
                }

                let close = time_price_bar.close();

                match time_price_bar.indicators() {
                    Some(Indicators {
                        ema: (ema, ema_slope),
                        bollinger_bands: Some((sma, upper_band, _)),
                    }) => {
                        if ema < sma {
                            // Ensure EMA is above SMA
                            Err(eyre!("EMA {:?} is below SMA {:?}", ema, sma))
                        } else if ema_slope.is_negative() {
                            // Ensure EMA slope is positive
                            Err(eyre!("EMA slope {:?} is negative", ema_slope))
                        } else if close < ema {
                            // Ensure close is above EMA
                            Err(eyre!("Close {:?} is below EMA {:?}", close, ema))
                        } else if close > upper_band {
                            // Ensure close is below upper band
                            Err(eyre!("Close {:?} is above upper band {:?}", close, upper_band))
                        } else if previous_trade_resolution_ts.is_some_and(
                            |previous_trade_resolution_ts| {
                                let mut previous_ema_sma_cross_ts = timestamp;
                                for (ts, time_price_bar) in pair_time_price_bars.data().iter().rev()
                                {
                                    if let Some(Indicators {
                                        ema: (ema, _),
                                        bollinger_bands: Some((sma, _, _)),
                                    }) = time_price_bar.indicators()
                                    {
                                        if ema < sma {
                                            break;
                                        } else {
                                            previous_ema_sma_cross_ts = ts;
                                        }
                                    }
                                }

                                previous_trade_resolution_ts >= *previous_ema_sma_cross_ts
                            },
                        ) {
                            // Ensure we did not previously close a trade on the same EMA -> SMA cross
                            Err(eyre!(
                                "Previous trade closed on the same EMA over SMA cross"
                            ))
                        } else {
                            debug!(
                                close = close.to_string(),
                                ema = ema.to_string(),
                                sma = sma.to_string(),
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
        resolution: &Resolution,
        open_trade_metadata: &TradeMetadata,
    ) -> Result<()> {
        let open_block_resolution_timestamp = &ResolutionTimestamp::from_timestamp(
            *open_trade_metadata.block_timestamp(),
            &resolution,
        );

        pair_time_price_bars
            .time_price_bar(now_block_resolution_timestamp)
            .ok_or_else(|| eyre!("No time price bar found for pair",))
            .and_then(|time_price_bar| {
                match time_price_bar.indicators() {
                    Some(Indicators {
                        bollinger_bands: Some((sma, _, _)),
                        ema: (ema, ema_slope),
                    }) => {
                        // Ensure that we did not open a position on the same time price bar
                        if now_block_resolution_timestamp == open_block_resolution_timestamp {
                            return Err(eyre!("Opened position on the same time price bar"));
                        }

                        // Ensure the most recent time price bar is negative
                        if time_price_bar
                            .data()
                            .map(|d| !d.is_negative())
                            .unwrap_or(true)
                        {
                            return Err(eyre!("Time price bar is not negative"));
                        }

                        // Close special case: If current close is below our initial entry price
                        /*
                        {
                            let entry_price = open_trade_metadata
                                .indexed_trade()
                                .token_price_before(open_trade_metadata.token_address());
                            let current_price = *time_price_bar.close();
                            if ema_slope.is_negative() && current_price < entry_price {
                                debug!(
                                    entry_price = entry_price.to_string(),
                                    current_price = current_price.to_string(),
                                    "should_close_position: current_price < entry_price"
                                );
                                return Ok(());
                            }
                        }
                        */

                        // Close special case: If the previous time price bar closed below the SMA
                        /*
                        {
                            let previous_resolution_timestamp =
                                now_block_resolution_timestamp.previous(&resolution);
                            let previous_time_price_bar = pair_time_price_bars
                                .time_price_bar(&previous_resolution_timestamp)
                                .ok_or_else(|| {
                                    eyre!("No previous time price bar found for pair")
                                })?;
                            let previous_close = previous_time_price_bar.close();
                            if previous_resolution_timestamp > *open_block_resolution_timestamp
                                && previous_close < sma
                            {
                                debug!(
                                    previous_close = previous_close.to_string(),
                                    sma = sma.to_string(),
                                    "should_close_position: previous_close < sma"
                                );
                                return Ok(());
                            }
                        }
                        */

                        // Ensure the EMA crossed below the SMA
                        if ema > sma {
                            return Err(eyre!("EMA {:?} is above SMA {:?}", ema, sma));
                        }

                        // All close conditions met, close the trade
                        debug!(
                            sma = sma.to_string(),
                            ema = ema.to_string(),
                            "should_close_position: close conditions met"
                        );
                        Ok(())
                    }
                    Some(_) => Err(eyre!("No bollinger bands found for pair.")),
                    None => Err(eyre!("No indicators found for pair.")),
                }
            })
    }
}
