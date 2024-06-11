use pochtecatl_primitives::{Resolution, ResolutionTimestamp, TimePriceBars, TradeMetadata};

use eyre::Result;

pub trait Strategy: Send + Sync + 'static {
    fn should_open_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        block_timestamp: u64,
        previous_close_trade_metadata: Option<&TradeMetadata>,
    ) -> Result<()>;

    fn should_close_position(
        &self,
        pair_time_price_bars: &TimePriceBars,
        block_timestamp: u64,
        open_trade_metadata: &TradeMetadata,
    ) -> Result<()>;
}
