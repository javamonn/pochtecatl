use crate::indexer::{ResolutionTimestamp, TimePriceBars};

use eyre::Result;

pub trait Strategy: Send + Sync + 'static {
    fn should_open_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        block_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<()>;

    fn should_close_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        now_block_resolution_timestamp: &ResolutionTimestamp,
        open_block_resolution_timestamp: &ResolutionTimestamp,
    ) -> Result<()>;
}
