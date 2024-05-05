use crate::{
    indexer::{
        IndexedUniswapV2Pair, Resolution, ResolutionTimestamp, TimePriceBars, UniswapV2PairTrade,
    },
    trade_controller::TradeMetadata,
};

use alloy::primitives::Address;

use eyre::Result;
use fnv::FnvHashMap;

pub trait UniswapV2Strategy: Send + Sync + 'static {
    fn should_open_position(
        &self,
        uniswap_v2_pair: &IndexedUniswapV2Pair,
        block_resolution_timestamp: &ResolutionTimestamp,
        time_price_bars: &FnvHashMap<Address, TimePriceBars>,
    ) -> Result<()>;

    fn should_close_position(
        &self,
        uniswap_v2_pair: &IndexedUniswapV2Pair,
        block_resolution_timestamp: &ResolutionTimestamp,
        resolution: &Resolution,
        open_trade_metadata: &TradeMetadata<UniswapV2PairTrade>,
        time_price_bars: &FnvHashMap<Address, TimePriceBars>,
    ) -> Result<()>;
}
