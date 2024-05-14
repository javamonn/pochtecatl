use super::{super::DexPairBlockTick, UniswapV3IndexedTrade, UniswapV3Pair};
use crate::{config, primitives::TickData};

use alloy::primitives::Address;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UniswapV3PairBlockTick {
    pub token_address: Address,
    pub makers: Vec<Address>,
    pub tick: TickData,
}

impl UniswapV3PairBlockTick {
    pub fn new(token_address: Address, makers: Vec<Address>, tick: TickData) -> Self {
        Self {
            token_address,
            makers,
            tick,
        }
    }
}

impl DexPairBlockTick<UniswapV3IndexedTrade, UniswapV3Pair> for UniswapV3PairBlockTick {
    fn tick(&self) -> &TickData {
        &self.tick
    }

    fn add_indexed_trade(&mut self, indexed_trade: &UniswapV3IndexedTrade) {
        self.makers.push(indexed_trade.maker);
        self.tick
            .add_indexed_trade(indexed_trade, &self.token_address);
    }

    fn from_indexed_trade(indexed_trade: UniswapV3IndexedTrade, token_address: Address) -> Self {
        Self::new(
            token_address,
            vec![indexed_trade.maker],
            TickData::from_indexed_trade(&indexed_trade, &token_address),
        )
    }

    fn pair(&self, pair_address: Address) -> UniswapV3Pair {
        let (token0, token1) = if self.token_address < *config::WETH_ADDRESS {
            (self.token_address, *config::WETH_ADDRESS)
        } else {
            (*config::WETH_ADDRESS, self.token_address)
        };

        UniswapV3Pair::new(pair_address, token0, token1)
    }
}
