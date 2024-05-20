use super::{
    super::{DexIndexedTrade, IndexedTradeParseContext},
    abi,
};

use crate::{
    config,
    primitives::{u32f96_from_sqrt_x96, FIXED_POINT_SCALE},
};

use alloy::{
    primitives::{Address, FixedBytes, Sign, Signed, U256, U512},
    sol_types::SolEvent,
};

use eyre::{OptionExt, Result};
use fixed::types::U32F96;
use lazy_static::lazy_static;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

lazy_static! {
    pub static ref PRICE_FACTOR: BigUint = BigUint::from(2_u128).pow(96);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3IndexedTrade {
    pub pair_address: Address,
    pub maker: Address,

    pub amount0: Signed<256, 4>,
    pub amount1: Signed<256, 4>,
    pub sqrt_price_x96: U256,
    pub liquidity: U256,
}

impl UniswapV3IndexedTrade {
    pub fn new(
        pair_address: Address,
        maker: Address,
        sqrt_price_x96: U256,
        liquidity: U256,
        amount0: Signed<256, 4>,
        amount1: Signed<256, 4>,
    ) -> Self {
        Self {
            pair_address,
            maker,
            sqrt_price_x96,
            liquidity,
            amount0,
            amount1,
        }
    }
}

impl DexIndexedTrade for UniswapV3IndexedTrade {
    fn event_signature_hashes() -> Vec<FixedBytes<32>> {
        vec![abi::uniswap_v3_pool::IUniswapV3Pool::Swap::SIGNATURE_HASH]
    }

    fn token_price_after(&self, token_address: &Address) -> U32F96 {
        u32f96_from_sqrt_x96(self.sqrt_price_x96, *token_address > *config::WETH_ADDRESS)
    }

    fn token_price_before(&self, token_address: &Address) -> U32F96 {
        let (mod_sign, mod_value) = if self.amount0.is_positive() {
            let (_, amount0_value) = self.amount0.into_sign_and_abs();
            (Sign::Positive, amount0_value)
        } else {
            let (_, amount1_value) = self.amount1.into_sign_and_abs();
            (Sign::Negative, amount1_value)
        };

        let adjusted_value = U256::from(
            (U512::from(mod_value) * *FIXED_POINT_SCALE)
                .checked_div(U512::from(self.liquidity))
                .unwrap_or_else(|| {
                    debug!(
                        "Failed to calculate adjusted value for UniswapV3IndexedTrade: {:?}",
                        token_address
                    );
                    U512::ZERO
                }),
        );

        u32f96_from_sqrt_x96(
            match mod_sign {
                Sign::Negative => self.sqrt_price_x96 - adjusted_value,
                Sign::Positive => self.sqrt_price_x96 + adjusted_value,
            },
            *token_address > *config::WETH_ADDRESS,
        )
    }

    fn weth_volume(&self, token_address: &Address) -> U256 {
        if *token_address < *config::WETH_ADDRESS {
            let (_, amount1_value) = self.amount1.into_sign_and_abs();
            amount1_value
        } else {
            let (_, amount0_value) = self.amount0.into_sign_and_abs();
            amount0_value
        }
    }

    fn pair_address(&self) -> &Address {
        &self.pair_address
    }
}

impl TryFrom<&IndexedTradeParseContext<'_>> for UniswapV3IndexedTrade {
    type Error = eyre::Report;

    fn try_from(value: &IndexedTradeParseContext) -> Result<Self, Self::Error> {
        value
            .logs()
            .get(value.idx())
            .and_then(|log| abi::uniswap_v3_pool::try_parse_swap_event(log))
            .ok_or_eyre("Failed to parse UniswapV3PoolSwapLog")
            .and_then(|parsed_swap| {
                if parsed_swap.liquidity == 0 {
                    warn!(
                        "Zero liquidity in UniswapV3IndexedTrade: {:?}",
                        value.logs().get(value.idx())
                    );
                }

                Ok(Self::new(
                    parsed_swap.address,
                    parsed_swap.recipient,
                    parsed_swap.sqrtPriceX96,
                    U256::from(parsed_swap.liquidity),
                    parsed_swap.amount0,
                    parsed_swap.amount1,
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config, primitives::IndexedTrade, providers::rpc_provider::new_http_signer_provider,
    };

    use alloy::primitives::{address, fixed_bytes};

    use eyre::{OptionExt, Result};

    #[tokio::test]
    async fn test_try_from_log() -> Result<()> {
        let rpc_provider = new_http_signer_provider(&config::RPC_URL, None).await?;
        let tx_receipt = rpc_provider
            .get_transaction_receipt(fixed_bytes!(
                "c8385640b305807c6bf58c9d55e3c7f0ffcb4ef1fb2abe641818f5925fc587fc"
            ))
            .await?
            .ok_or_eyre("Failed to get transaction receipt")?;

        let receipt = tx_receipt
            .as_ref()
            .as_receipt()
            .ok_or_eyre("Failed to convert TransactionReceipt to Receipt")?;

        let trades = IndexedTrade::from_logs(&receipt.logs);

        assert_eq!(trades.len(), 1);

        let address = address!("4ed4E862860beD51a9570b96d89aF5E1B0Efefed");
        let before_price = trades[0].token_price_before(&address);
        let after_price = trades[0].token_price_after(&address);

        assert_eq!(before_price.to_string(), "0.0000056102519540739871334719");
        assert_eq!(after_price.to_string(), "0.00000561025196139503809128204");
        assert!(before_price < after_price);

        Ok(())
    }
}
