use super::{
    super::{DexIndexedTrade, IndexedTradeParseContext},
    abi,
};

use crate::{config, primitives::u32f96_from_u256_frac};

use alloy::{
    primitives::{Address, FixedBytes, U256},
    sol_types::SolEvent,
};

use eyre::{OptionExt, Result};
use fixed::types::U32F96;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct UniswapV2IndexedTrade {
    pub pair_address: Address,
    pub amount0_in: U256,
    pub amount1_in: U256,
    pub amount0_out: U256,
    pub amount1_out: U256,
    pub reserve0: U256,
    pub reserve1: U256,
    pub maker: Address,
}

impl UniswapV2IndexedTrade {
    pub fn new(
        pair_address: Address,
        amount0_in: U256,
        amount1_in: U256,
        amount0_out: U256,
        amount1_out: U256,
        reserve0: U256,
        reserve1: U256,
        maker: Address,
    ) -> Self {
        Self {
            pair_address,
            amount0_in,
            amount1_in,
            amount0_out,
            amount1_out,
            reserve0,
            reserve1,
            maker,
        }
    }
}

impl DexIndexedTrade for UniswapV2IndexedTrade {
    fn event_signature_hashes() -> Vec<FixedBytes<32>> {
        vec![
            abi::uniswap_v2_pair::IUniswapV2Pair::Sync::SIGNATURE_HASH,
            abi::uniswap_v2_pair::IUniswapV2Pair::Swap::SIGNATURE_HASH,
        ]
    }

    fn pair_address(&self) -> &Address {
        &self.pair_address
    }

    fn weth_volume(&self, token_address: &Address) -> U256 {
        if *token_address < *config::WETH_ADDRESS {
            // token0 is token
            self.amount1_in + self.amount1_out
        } else {
            // token1 is token
            self.amount0_in + self.amount0_out
        }
    }

    fn token_price_before(&self, token_address: &Address) -> U32F96 {
        let reserve0_before = self.reserve0 - self.amount0_in + self.amount0_out;
        let reserve1_before = self.reserve1 - self.amount1_in + self.amount1_out;

        if *token_address < *config::WETH_ADDRESS {
            u32f96_from_u256_frac(reserve1_before, reserve0_before)
        } else {
            u32f96_from_u256_frac(reserve0_before, reserve1_before)
        }
    }

    fn token_price_after(&self, token_address: &Address) -> U32F96 {
        if *token_address < *config::WETH_ADDRESS {
            u32f96_from_u256_frac(self.reserve1, self.reserve0)
        } else {
            u32f96_from_u256_frac(self.reserve0, self.reserve1)
        }
    }
}

impl TryFrom<&IndexedTradeParseContext<'_>> for UniswapV2IndexedTrade {
    type Error = eyre::Report;

    fn try_from(value: &IndexedTradeParseContext) -> Result<Self, Self::Error> {
        value
            .logs()
            .get(value.idx())
            .zip(
                value
                    .idx()
                    .checked_sub(1)
                    .and_then(|prev_idx| value.logs().get(prev_idx)),
            )
            .and_then(|(log, prev_log)| {
                // Ensure prev log in the arr is the previous log index in the
                // same block for the same pair
                if prev_log.block_hash == log.block_hash
                    && prev_log.address() == log.address()
                    && prev_log.log_index.is_some_and(|prev_log_index| {
                        log.log_index
                            .is_some_and(|log_index| prev_log_index + 1 == log_index)
                    })
                {
                    abi::uniswap_v2_pair::try_parse_swap_event(&log)
                        .zip(abi::uniswap_v2_pair::try_parse_sync_event(&prev_log))
                } else {
                    None
                }
            })
            .map(|(swap_log, sync_log)| {
                Self::new(
                    swap_log.address,
                    swap_log.amount0In,
                    swap_log.amount1In,
                    swap_log.amount0Out,
                    swap_log.amount1Out,
                    U256::from(sync_log.reserve0),
                    U256::from(sync_log.reserve1),
                    swap_log.to,
                )
            })
            .ok_or_eyre("Failed to parse UniswapV2IndexedTrade")
    }
}
