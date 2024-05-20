use super::{super::DexPairBlockTick, UniswapV3IndexedTrade, UniswapV3Pair};
use crate::{
    config,
    primitives::{DexPair, TickData},
};

use alloy::primitives::Address;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UniswapV3PairBlockTick {
    pub pair: UniswapV3Pair,
    pub makers: Vec<Address>,
    pub tick: TickData,
}

impl DexPairBlockTick<UniswapV3IndexedTrade, UniswapV3Pair> for UniswapV3PairBlockTick {
    fn new(indexed_trade: UniswapV3IndexedTrade, pair: UniswapV3Pair) -> Self {
        Self {
            pair,
            tick: TickData::from_indexed_trade(&indexed_trade, pair.token_address()),
            makers: vec![indexed_trade.maker],
        }
    }

    fn tick(&self) -> &TickData {
        &self.tick
    }

    fn add_indexed_trade(&mut self, indexed_trade: &UniswapV3IndexedTrade) {
        self.makers.push(indexed_trade.maker);
        self.tick
            .add_indexed_trade(indexed_trade, self.pair.token_address());
    }

    fn pair(&self) -> &UniswapV3Pair {
        &self.pair
    }
}
