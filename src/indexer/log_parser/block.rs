use crate::config;

use alloy::{
    primitives::{Address, BlockHash, BlockNumber, U256},
    rpc::types::eth::Header,
};
use eyre::eyre;
use fnv::FnvHashMap;
use fraction::GenericFraction;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UniswapV2PairTrade {
    pub amount0_in: U256,
    pub amount1_in: U256,
    pub amount0_out: U256,
    pub amount1_out: U256,
    pub reserve0: U256,
    pub reserve1: U256,
    pub maker: Address,
}

impl UniswapV2PairTrade {
    pub fn new(
        amount0_in: U256,
        amount1_in: U256,
        amount0_out: U256,
        amount1_out: U256,
        reserve0: U256,
        reserve1: U256,
        maker: Address,
    ) -> Self {
        Self {
            amount0_in,
            amount1_in,
            amount0_out,
            amount1_out,
            reserve0,
            reserve1,
            maker,
        }
    }

    pub fn get_price_before(
        &self,
        token0_address: &Address,
        token1_address: &Address,
    ) -> Option<GenericFraction<u128>> {
        let reserve0_before = self.reserve0 - self.amount0_in + self.amount0_out;
        let reserve1_before = self.reserve1 - self.amount1_in + self.amount1_out;
        if *token0_address == *config::WETH_ADDRESS {
            Some(GenericFraction::new(
                reserve0_before.to::<u128>(),
                reserve1_before.to::<u128>(),
            ))
        } else if *token1_address == *config::WETH_ADDRESS {
            Some(GenericFraction::new(
                reserve1_before.to::<u128>(),
                reserve0_before.to::<u128>(),
            ))
        } else {
            None
        }
    }

    pub fn get_price_after(
        &self,
        token0_address: &Address,
        token1_address: &Address,
    ) -> Option<GenericFraction<u128>> {
        if *token0_address == *config::WETH_ADDRESS {
            Some(GenericFraction::new(
                self.reserve0.to::<u128>(),
                self.reserve1.to::<u128>(),
            ))
        } else if *token1_address == *config::WETH_ADDRESS {
            Some(GenericFraction::new(
                self.reserve1.to::<u128>(),
                self.reserve0.to::<u128>(),
            ))
        } else {
            None
        }
    }
}

pub struct Block {
    pub block_hash: BlockHash,
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub uniswap_v2_trades: FnvHashMap<Address, Vec<UniswapV2PairTrade>>,
}

impl Block {
    pub fn new(block_hash: BlockHash, block_number: BlockNumber, block_timestamp: u64) -> Self {
        Self {
            block_hash,
            block_number,
            block_timestamp,
            uniswap_v2_trades: FnvHashMap::default(),
        }
    }
}

impl TryFrom<&Header> for Block {
    type Error = eyre::Report;

    fn try_from(header: &Header) -> Result<Self, Self::Error> {
        match (header.hash, header.number) {
            (None, _) => Err(eyre!("header is missing hash")),
            (_, None) => Err(eyre!("header is missing number")),
            (Some(hash), Some(number)) => Ok(Block::new(
                hash,
                number.to::<u64>(),
                header.timestamp.to::<u64>(),
            )),
        }
    }
}
