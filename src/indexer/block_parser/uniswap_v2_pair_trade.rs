use super::{uniswap_v2_pair_swap_log, uniswap_v2_pair_sync_log, ParseableTrade};

use crate::config;

use alloy::{
    primitives::{Address, U256},
    rpc::types::eth::Log,
};

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

    pub fn get_price_before(&self, token_address: &Address) -> GenericFraction<u128> {
        let reserve0_before = self.reserve0 - self.amount0_in + self.amount0_out;
        let reserve1_before = self.reserve1 - self.amount1_in + self.amount1_out;

        if *token_address < *config::WETH_ADDRESS {
            // token0 is token, token1 is weth
            GenericFraction::new(reserve1_before.to::<u128>(), reserve0_before.to::<u128>())
        } else {
            // token0 is weth, token1 is token
            GenericFraction::new(reserve0_before.to::<u128>(), reserve1_before.to::<u128>())
        }
    }

    pub fn get_price_after(&self, token_address: &Address) -> GenericFraction<u128> {
        if *token_address < *config::WETH_ADDRESS {
            // token0 is token, token1 is weth
            GenericFraction::new(self.reserve1.to::<u128>(), self.reserve0.to::<u128>())
        } else {
            // token0 is weth, token1 is token
            GenericFraction::new(self.reserve0.to::<u128>(), self.reserve1.to::<u128>())
        }
    }
}

impl ParseableTrade for UniswapV2PairTrade {
    fn parse_from_log(
        log: &Log,
        logs: &Vec<Log>,
        relative_log_idx: usize,
    ) -> Option<UniswapV2PairTrade> {
        uniswap_v2_pair_swap_log::parse(&log).and_then(|parsed_swap| {
            relative_log_idx
                .checked_sub(1)
                .and_then(|prev_log_idx| logs.get(prev_log_idx))
                .and_then(|prev_log| {
                    // Ensure prev log in the arr is the previous log index in the
                    // same block for the same pair
                    if prev_log.block_hash == log.block_hash
                        && prev_log.address() == log.address()
                        && prev_log.log_index.is_some_and(|prev_log_index| {
                            log.log_index
                                .is_some_and(|log_index| prev_log_index + 1 == log_index)
                        })
                    {
                        uniswap_v2_pair_sync_log::parse(&prev_log)
                    } else {
                        None
                    }
                })
                .map(|parsed_sync| {
                    UniswapV2PairTrade::new(
                        parsed_swap.amount0In,
                        parsed_swap.amount1In,
                        parsed_swap.amount0Out,
                        parsed_swap.amount1Out,
                        U256::from(parsed_sync.reserve0),
                        U256::from(parsed_sync.reserve1),
                        parsed_swap.to,
                    )
                })
        })
    }
}
