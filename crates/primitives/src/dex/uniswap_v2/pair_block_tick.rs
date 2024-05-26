use super::{super::DexPairBlockTick, UniswapV2IndexedTrade, UniswapV2Pair};
use crate::{constants, DexPair, TickData};

use alloy::primitives::{Address, U256};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UniswapV2PairBlockTick {
    pub pair: UniswapV2Pair,
    pub makers: Vec<Address>,
    pub tick: TickData,

    // Reserves at the end of the block
    pub reserve0: U256,
    pub reserve1: U256,
}

impl UniswapV2PairBlockTick {
    // ordered as token reserve, weth reserve
    fn reserves(&self) -> (U256, U256) {
        if *self.pair.token_address() < constants::WETH_ADDRESS {
            (self.reserve0, self.reserve1)
        } else {
            (self.reserve1, self.reserve0)
        }
    }
}

impl DexPairBlockTick<UniswapV2IndexedTrade, UniswapV2Pair> for UniswapV2PairBlockTick {
    fn tick(&self) -> &TickData {
        &self.tick
    }

    fn add_indexed_trade(&mut self, indexed_trade: &UniswapV2IndexedTrade) {
        self.reserve0 = indexed_trade.reserve0;
        self.reserve1 = indexed_trade.reserve1;
        self.makers.push(indexed_trade.maker);
        self.tick
            .add_indexed_trade(indexed_trade, &self.pair.token_address());
    }

    fn new(indexed_trade: UniswapV2IndexedTrade, pair: UniswapV2Pair) -> Self {
        Self {
            pair,
            tick: TickData::from_indexed_trade(&indexed_trade, pair.token_address()),
            reserve0: indexed_trade.reserve0,
            reserve1: indexed_trade.reserve1,
            makers: vec![indexed_trade.maker],
        }
    }

    fn pair(&self) -> &UniswapV2Pair {
        &self.pair
    }
}
