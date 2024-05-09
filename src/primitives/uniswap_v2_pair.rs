use super::UniswapV2PairTrade;

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct UniswapV2Pair {
    pub token_address: Address,
    pub trades: Vec<UniswapV2PairTrade>,
}

impl UniswapV2Pair {
    pub fn new(token_address: Address, trades: Vec<UniswapV2PairTrade>) -> Self {
        Self {
            token_address,
            trades,
        }
    }
}
