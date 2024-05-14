use super::{super::DexPairBlockTick, UniswapV2IndexedTrade, UniswapV2Pair};
use crate::{config, primitives::TickData};

use alloy::primitives::{Address, U256};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UniswapV2PairBlockTick {
    pub token_address: Address,
    pub makers: Vec<Address>,
    pub tick: TickData,

    // Reserves at the end of the block
    pub reserve0: U256,
    pub reserve1: U256,
}

impl UniswapV2PairBlockTick {
    pub fn new(
        token_address: Address,
        makers: Vec<Address>,
        tick: TickData,
        reserve0: U256,
        reserve1: U256,
    ) -> Self {
        Self {
            token_address,
            makers,
            tick,
            reserve0,
            reserve1,
        }
    }

    // ordered as token reserve, weth reserve
    fn reserves(&self) -> (U256, U256) {
        if self.token_address < *config::WETH_ADDRESS {
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
            .add_indexed_trade(indexed_trade, &self.token_address);
    }

    fn from_indexed_trade(indexed_trade: UniswapV2IndexedTrade, token_address: Address) -> Self {
        Self::new(
            token_address,
            vec![indexed_trade.maker],
            TickData::from_indexed_trade(&indexed_trade, &token_address),
            indexed_trade.reserve0,
            indexed_trade.reserve1,
        )
    }

    fn pair(&self, pair_address: Address) -> UniswapV2Pair {
        let (token0, token1) = if self.token_address < *config::WETH_ADDRESS {
            (self.token_address, *config::WETH_ADDRESS)
        } else {
            (*config::WETH_ADDRESS, self.token_address)
        };

        UniswapV2Pair::new(pair_address, token0, token1)
    }
}
