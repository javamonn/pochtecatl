use super::Strategy;
use pochtecatl_primitives::{Indicators, ResolutionTimestamp, TimePriceBars, TradeMetadata};

use eyre::{eyre, Result};
use tracing::debug;

pub struct MomentumStrategy {}

pub const TRADE_DEBOUNCE_SECONDS: u64 = 60 * 60;

impl MomentumStrategy {
    pub fn new() -> Self {
        Self {}
    }
}

impl Strategy for MomentumStrategy {
    fn should_open_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        block_timestamp: u64,
        previous_close_trade_metadata: Option<&TradeMetadata>,
    ) -> Result<()> {
        if previous_close_trade_metadata.is_some_and(|previous_close_trade_metadata| {
            block_timestamp - previous_close_trade_metadata.block_timestamp()
                < TRADE_DEBOUNCE_SECONDS
        }) {
            return Err(eyre!("Trade debounce period not met"));
        }

        let resolution_timestamp =
            ResolutionTimestamp::from_timestamp(block_timestamp, pair_time_price_bars.resolution());

        let time_price_bar = pair_time_price_bars
            .data()
            .get(&resolution_timestamp)
            .ok_or_else(|| eyre!("Failed to get time price bar"))?;

        match time_price_bar.indicators() {
            Some(Indicators {
                bollinger_bands: Some((sma, _, _, _)),
                ema: (ema, ema_slope),
            }) => {
                if ema < sma {
                    return Err(eyre!("EMA {} is below SMA {}", ema, sma));
                }
                if ema_slope.is_negative() {
                    return Err(eyre!("EMA slope {} is negative", ema_slope));
                }

                // If we've already traded this ema -> sma cross, we should not open a new
                // position
                let have_traded_cross = previous_close_trade_metadata
                    .map(|metadata| {
                        // The resolution timestamp of the previous close trade
                        let previous_close_resolution_ts = ResolutionTimestamp::from_timestamp(
                            *metadata.block_timestamp(),
                            pair_time_price_bars.resolution(),
                        );
                        // The most recent resolution timestamp where the EMA crossed above the SMA
                        let last_ema_sma_cross = {
                            let mut previous_ema_sma_cross_ts = &resolution_timestamp;
                            for (ts, time_price_bar) in pair_time_price_bars.data().iter().rev() {
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
                            previous_ema_sma_cross_ts
                        };

                        previous_close_resolution_ts >= *last_ema_sma_cross
                    })
                    .unwrap_or(false);

                if have_traded_cross {
                    return Err(eyre!("Have traded EMA over SMA cross"));
                }

                Ok(())
            }
            _ => Err(eyre!("Failed to get indicators")),
        }
    }

    fn should_close_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        block_timestamp: u64,
        open_trade_metadata: &TradeMetadata,
    ) -> Result<()> {
        let resolution_timestamp =
            ResolutionTimestamp::from_timestamp(block_timestamp, pair_time_price_bars.resolution());
        let time_price_bar = pair_time_price_bars
            .data()
            .get(&resolution_timestamp)
            .ok_or_else(|| eyre!("Failed to get time price bar"))?;

        if block_timestamp - open_trade_metadata.block_timestamp() < TRADE_DEBOUNCE_SECONDS {
            return Err(eyre!("Trade debounce period not met"));
        }

        match time_price_bar.indicators() {
            Some(Indicators {
                bollinger_bands: Some((sma, _, _, _)),
                ema: (ema, ema_slope),
            }) => {
                if ema_slope.is_negative() && ema < sma {
                    debug!("closing: ema {} is below sma {}", ema, sma);
                    return Ok(());
                }

                // Stop loss
                if open_trade_metadata
                    .indexed_trade()
                    .token_price_before(open_trade_metadata.token_address())
                    < *time_price_bar.close()
                {
                    debug!("closing: stop loss");
                    return Ok(());
                }

                // Close conditions not met
                return Err(eyre!("Close conditions not met"));
            }
            _ => Err(eyre!("Failed to get indicators")),
        }
    }
}
