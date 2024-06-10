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
                        bollinger_bands: Some((sma, upper_band, _, sma_slope)),
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
                        } else if sma_slope.is_negative() {
                            // Ensure SMA slope is positive
                            Err(eyre!("SMA slope {:?} is negative", sma_slope))
                        } else if previous_trade_resolution_ts.is_some_and(
                            |previous_trade_resolution_ts| {
                                let mut previous_ema_sma_cross_ts = timestamp;
                                for (ts, time_price_bar) in pair_time_price_bars.data().iter().rev()
                                {
                                    if let Some(Indicators {
                                        ema: (ema, _),
                                        bollinger_bands: Some((sma, _, _, _)),
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
                        bollinger_bands: Some((sma, _, _, sma_slope)),
                        ema: (ema, ema_slope),
                    }) => {
                        // Ensure the most recent time price bar is negative
                        if time_price_bar
                            .data()
                            .map(|d| !d.is_negative())
                            .unwrap_or(true)
                        {
                            return Err(eyre!("Time price bar is not negative"));
                        }

                        // Close if EMA is below SMA
                        if ema < sma {
                            return Ok(());
                        }

                        // Close if Close is below SMA and prev SMA slope closed negative
                        if time_price_bar.close() < sma
                            && pair_time_price_bars
                                .data()
                                .get(
                                    &now_block_resolution_timestamp
                                        .previous(resolution)
                                        .max(*open_block_resolution_timestamp),
                                )
                                .and_then(|t| t.indicators())
                                .and_then(|i| i.bollinger_bands)
                                .is_some_and(|(_, _, _, prev_sma_slope)| {
                                    prev_sma_slope.is_negative()
                                })
                        {
                            return Ok(());
                        }

                        Err(eyre!("Should close position"))
                    }
                    Some(_) => Err(eyre!("No bollinger bands found for pair.")),
                    None => Err(eyre!("No indicators found for pair.")),
                }
            })
    }
}
