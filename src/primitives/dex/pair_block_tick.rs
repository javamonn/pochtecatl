use super::{DexIndexedTrade, IndexedTrade, Pair, UniswapV2PairBlockTick, UniswapV3PairBlockTick};
use crate::primitives::TickData;

use alloy::primitives::Address;

use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum PairBlockTick {
    UniswapV2(UniswapV2PairBlockTick),
    UniswapV3(UniswapV3PairBlockTick),
}

pub trait DexPairBlockTick<T: DexIndexedTrade, P: Into<Pair>> {
    fn tick(&self) -> &TickData;
    fn add_indexed_trade(&mut self, indexed_trade: &T);
    fn from_indexed_trade(indexed_trade: T, token_address: Address) -> Self;
    fn pair(&self, pair_address: Address) -> P;
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

    pub fn from_indexed_trade(indexed_trade: IndexedTrade, token_address: Address) -> Self {
        match indexed_trade {
            IndexedTrade::UniswapV2(indexed_trade) => Self::UniswapV2(
                UniswapV2PairBlockTick::from_indexed_trade(indexed_trade, token_address),
            ),
            IndexedTrade::UniswapV3(indexed_trade) => Self::UniswapV3(
                UniswapV3PairBlockTick::from_indexed_trade(indexed_trade, token_address),
            ),
        }
    }

    pub fn pair(&self, pair_address: Address) -> Pair {
        match self {
            Self::UniswapV2(pair_block_tick) => pair_block_tick.pair(pair_address).into(),
            Self::UniswapV3(pair_block_tick) => pair_block_tick.pair(pair_address).into(),
        }
    }
}
