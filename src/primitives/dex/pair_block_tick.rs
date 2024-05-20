use super::{
    DexIndexedTrade, DexPair, IndexedTrade, Pair, UniswapV2PairBlockTick, UniswapV3PairBlockTick,
};
use crate::primitives::TickData;

use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum PairBlockTick {
    UniswapV2(UniswapV2PairBlockTick),
    UniswapV3(UniswapV3PairBlockTick),
}

pub trait DexPairBlockTick<T: DexIndexedTrade + Into<IndexedTrade>, P: DexPair<T> + Into<Pair>> {
    fn new(indexed_trade: T, pair: P) -> Self;
    fn tick(&self) -> &TickData;
    fn add_indexed_trade(&mut self, indexed_trade: &T);
    fn pair(&self) -> &P;
}

impl PairBlockTick {
    pub fn tick(&self) -> &TickData {
        match self {
            Self::UniswapV2(pair_block_tick) => pair_block_tick.tick(),
            Self::UniswapV3(pair_block_tick) => pair_block_tick.tick(),
        }
    }

    pub fn add_indexed_trade(&mut self, indexed_trade: &IndexedTrade) -> Result<()> {
        match (self, indexed_trade) {
            (Self::UniswapV2(pair_block_tick), IndexedTrade::UniswapV2(indexed_trade)) => {
                pair_block_tick.add_indexed_trade(indexed_trade);
                Ok(())
            }
            (Self::UniswapV3(pair_block_tick), IndexedTrade::UniswapV3(indexed_trade)) => {
                pair_block_tick.add_indexed_trade(indexed_trade);
                Ok(())
            }
            _ => Err(eyre!("Invalid indexed trade for pair block tick")),
        }
    }

    pub fn new(indexed_trade: IndexedTrade, pair: Pair) -> Result<Self> {
        match (indexed_trade, pair) {
            (IndexedTrade::UniswapV2(indexed_trade), Pair::UniswapV2(pair)) => {
                Ok(UniswapV2PairBlockTick::new(indexed_trade, pair).into())
            }
            (IndexedTrade::UniswapV3(indexed_trade), Pair::UniswapV3(pair)) => {
                Ok(UniswapV3PairBlockTick::new(indexed_trade, pair).into())
            }
            _ => Err(eyre!("Invalid indexed trade for pair",)),
        }
    }

    pub fn pair(&self) -> Pair {
        match self {
            Self::UniswapV2(pair_block_tick) => pair_block_tick.pair().clone().into(),
            Self::UniswapV3(pair_block_tick) => pair_block_tick.pair().clone().into(),
        }
    }
}

impl From<UniswapV2PairBlockTick> for PairBlockTick {
    fn from(pair_block_tick: UniswapV2PairBlockTick) -> Self {
        Self::UniswapV2(pair_block_tick)
    }
}

impl From<UniswapV3PairBlockTick> for PairBlockTick {
    fn from(pair_block_tick: UniswapV3PairBlockTick) -> Self {
        Self::UniswapV3(pair_block_tick)
    }
}
