use super::{UniswapV2Pair, UniswapV2PairTrade};
use crate::config;

use alloy::primitives::{Address, BlockHash, BlockNumber};

use fnv::FnvHashMap;

pub struct Block {
    pub block_hash: Option<BlockHash>,
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub uniswap_v2_pairs: FnvHashMap<Address, UniswapV2Pair>,
}

impl Block {
    pub fn new(
        block_hash: Option<BlockHash>,
        block_number: BlockNumber,
        block_timestamp: u64,
    ) -> Self {
        Self {
            block_hash,
            block_number,
            block_timestamp,
            uniswap_v2_pairs: FnvHashMap::default(),
        }
    }

    pub fn add_uniswap_v2_pair_trade(
        &mut self,
        pair_address: Address,
        trade: UniswapV2PairTrade,
        token0_address: &Address,
        token1_address: &Address,
    ) {
        let token_address = if *token0_address == *config::WETH_ADDRESS {
            Some(token1_address)
        } else if *token1_address == *config::WETH_ADDRESS {
            Some(token0_address)
        } else {
            None
        };

        if let Some(token_address) = token_address {
            let uniswap_v2_pair = self
                .uniswap_v2_pairs
                .entry(pair_address)
                .or_insert_with(|| UniswapV2Pair::new(*token_address, Vec::new()));
            uniswap_v2_pair.trades.push(trade);
        }
    }
}

impl From<crate::db::BlockModel> for Block {
    fn from(value: crate::db::BlockModel) -> Self {
        Self {
            block_hash: None,
            block_number: value.number.into(),
            block_timestamp: value.timestamp.into(),
            uniswap_v2_pairs: serde_json::from_value(value.uniswap_v2_pairs).unwrap(),
        }
    }
}
