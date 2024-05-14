use crate::{config, primitives::Block};
use alloy::primitives::{Address, BlockNumber, U256};

pub struct Pair {
    pub token_reserve: U256,
    pub weth_reserve: U256,
    pub token_address: Address,
    pub pair_address: Address,
}

impl Pair {
    pub fn new(
        token_reserve: U256,
        weth_reserve: U256,
        token_address: Address,
        pair_address: Address,
    ) -> Self {
        Self {
            token_reserve,
            weth_reserve,
            token_address,
            pair_address,
        }
    }
}

pub struct BlockMessage {
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub pairs: Vec<Pair>,
}

impl BlockMessage {
    pub fn new(block_number: BlockNumber, block_timestamp: u64, pairs: Vec<Pair>) -> Self {
        Self {
            block_number,
            block_timestamp,
            pairs,
        }
    }
}

impl From<&Block> for BlockMessage {
    fn from(value: &Block) -> Self {
        Self::new(
            value.block_number,
            value.block_timestamp,
            value
                .pair_ticks
                .iter()
                .filter_map(|(pair_address, pair_tick)| match .last() {
                    Some(trade) => {
                        let (token_reserve, weth_reserve) =
                            if pair.token_address < *config::WETH_ADDRESS {
                                (trade.reserve0, trade.reserve1)
                            } else {
                                (trade.reserve1, trade.reserve0)
                            };

                        Some(IndexedUniswapV2Pair::new(
                            token_reserve,
                            weth_reserve,
                            pair.token_address,
                            *pair_address,
                        ))
                    }
                    None => None,
                })
                .collect(),
        )
    }
}
