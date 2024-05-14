use crate::{config, primitives::PairTick};
use alloy::primitives::{Address, U256};

#[derive(Debug)]
pub enum PairMessage {
    UniswapV2(UniswapV2PairMessage),
    UniswapV3(UniswapV3PairMessage),
}

impl PairMessage {
    pub fn pair_address(&self) -> &Address {
        match self {
            PairMessage::UniswapV2(pair) => &pair.pair_address,
            PairMessage::UniswapV3(pair) => &pair.pair_address,
        }
    }

    pub fn from_pair_tick(pair_address: Address, pair_tick: PairTick) -> Self {
        match pair_tick {
            PairTick::UniswapV2(pair_tick) => {
                let (token_reserve, weth_reserve) =
                    if pair_tick.token_address < *config::WETH_ADDRESS {
                        (pair_tick.reserve0, pair_tick.reserve1)
                    } else {
                        (pair_tick.reserve1, pair_tick.reserve0)
                    };
                PairMessage::UniswapV2(UniswapV2PairMessage::new(
                    token_reserve,
                    weth_reserve,
                    pair_tick.token_address,
                    pair_address,
                ))
            }
            PairTick::UniswapV3(pair_tick) => PairMessage::UniswapV3(UniswapV3PairMessage::new(
                pair_tick.token_address,
                pair_address,
            )),
        }
    }
}
