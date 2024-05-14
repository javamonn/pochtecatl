use super::{
    super::{DexIndexedTrade, IndexedTradeParseContext},
    abi,
};

use crate::config;

use alloy::{
    primitives::{Address, FixedBytes},
    sol_types::SolEvent,
};

use eyre::{OptionExt, Result};
use fraction::{GenericFraction, One};
use lazy_static::lazy_static;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};

lazy_static! {
    pub static ref PRICE_FACTOR: BigUint = BigUint::from(2_u128).pow(96);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3IndexedTrade {
    pub pair_address: Address,
    pub maker: Address,
    pub abs_amount0: BigUint,
    pub abs_amount1: BigUint,

    // decoded prices of token0 in token1, i.e. token1/token0
    pub price_after: GenericFraction<BigUint>,
    pub price_before: GenericFraction<BigUint>,
}

impl UniswapV3IndexedTrade {
    pub fn new(
        pair_address: Address,
        maker: Address,
        price_after: GenericFraction<BigUint>,
        price_before: GenericFraction<BigUint>,
        abs_amount0: BigUint,
        abs_amount1: BigUint,
    ) -> Self {
        Self {
            pair_address,
            maker,
            price_after,
            price_before,
            abs_amount0,
            abs_amount1,
        }
    }
}

impl DexIndexedTrade for UniswapV3IndexedTrade {
    fn event_signature_hashes() -> Vec<FixedBytes<32>> {
        vec![abi::uniswap_v3_pool::IUniswapV3Pool::Swap::SIGNATURE_HASH]
    }

    fn token_price_after(&self, token_address: &Address) -> GenericFraction<BigUint> {
        if *token_address < *config::WETH_ADDRESS {
            // token0 is token, token1 is weth
            self.price_after.clone()
        } else {
            // token1 is weth, token0 is token
            self.price_after.recip()
        }
    }

    fn token_price_before(&self, token_address: &Address) -> GenericFraction<BigUint> {
        if *token_address < *config::WETH_ADDRESS {
            // token0 is token, token1 is weth
            self.price_before.clone()
        } else {
            // token1 is weth, token0 is token
            self.price_before.recip()
        }
    }

    fn weth_volume(&self, token_address: &Address) -> BigUint {
        if *token_address < *config::WETH_ADDRESS {
            self.abs_amount1.clone()
        } else {
            self.abs_amount0.clone()
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
                let sqrt_price_x96: BigUint = parsed_swap.sqrtPriceX96.try_into().unwrap();

                let price_after = {
                    let sqrt_price =
                        GenericFraction::new(sqrt_price_x96.clone(), PRICE_FACTOR.clone());
                    sqrt_price.clone() * sqrt_price
                };

                let price_before = {
                    let sqrt_price_x96: GenericFraction<BigUint> =
                        GenericFraction::new(sqrt_price_x96, BigUint::one());
                    let liquidity = BigUint::from(parsed_swap.liquidity);

                    let sqrt_price_x96_before = if parsed_swap.amount0.is_positive() {
                        let amount0: BigUint = {
                            let (_, amount0_value) = parsed_swap.amount0.into_sign_and_abs();

                            amount0_value.try_into()?
                        };

                        sqrt_price_x96
                            + GenericFraction::new(amount0 * PRICE_FACTOR.clone(), liquidity)
                    } else {
                        let amount1: BigUint = {
                            let (_, amount1_value) = parsed_swap.amount1.into_sign_and_abs();
                            amount1_value.try_into()?
                        };

                        sqrt_price_x96
                            - GenericFraction::new(amount1 * PRICE_FACTOR.clone(), liquidity)
                    };

                    let sqrt_price = sqrt_price_x96_before / PRICE_FACTOR.clone();

                    sqrt_price.clone() * sqrt_price
                };

                Ok(Self::new(
                    parsed_swap.address,
                    parsed_swap.recipient,
                    price_after,
                    price_before,
                    BigUint::from(parsed_swap.amount0.into_sign_and_abs().1),
                    BigUint::from(parsed_swap.amount1.into_sign_and_abs().1),
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
    use fraction::GenericDecimal;
    use num_bigint::BigUint;

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

        assert_eq!(
            format!(
                "{}",
                GenericDecimal::<BigUint, usize>::from_fraction(before_price.clone())
                    .set_precision(18)
            ),
            "0.000005610251954073"
        );
        assert_eq!(
            format!(
                "{}",
                GenericDecimal::<BigUint, usize>::from_fraction(after_price.clone())
                    .set_precision(18)
            ),
            "0.000005610251961395"
        );
        assert!(before_price < after_price);

        Ok(())
    }
}
