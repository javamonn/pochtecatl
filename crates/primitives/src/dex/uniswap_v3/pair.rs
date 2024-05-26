use super::{
    super::{DexPair, DexPairInput, IndexedTrade, TradeRequestOp},
    abi, UniswapV3IndexedTrade,
};

use crate::{
    abi::multicall3::{self, multicall_tx_request},
    constants, RpcProvider,
};

use alloy::{
    network::{Ethereum, TransactionBuilder},
    primitives::{uint, Address, BlockNumber, Signed, TxKind, U256},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    sol_types::SolCall,
    transports::Transport,
};
use core::ops::Neg;
use eyre::{eyre, OptionExt, Result, WrapErr};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct UniswapV3PairInput(Address);

impl DexPairInput<UniswapV3Pair> for UniswapV3PairInput {
    fn address(&self) -> &Address {
        &self.0
    }

    fn calls(&self) -> Vec<multicall3::Call3> {
        vec![
            multicall3::Call3 {
                target: self.0,
                allowFailure: true,
                callData: abi::uniswap_v3_pool::IUniswapV3Pool::token0Call {}
                    .abi_encode()
                    .into(),
            },
            multicall3::Call3 {
                target: self.0,
                allowFailure: true,
                callData: abi::uniswap_v3_pool::IUniswapV3Pool::token1Call {}
                    .abi_encode()
                    .into(),
            },
            multicall3::Call3 {
                target: self.0,
                allowFailure: true,
                callData: abi::uniswap_v3_pool::IUniswapV3Pool::feeCall {}
                    .abi_encode()
                    .into(),
            },
            multicall3::Call3 {
                target: self.0,
                allowFailure: true,
                callData: abi::uniswap_v3_pool::IUniswapV3Pool::factoryCall {}
                    .abi_encode()
                    .into(),
            },
        ]
    }

    fn decode(&self, result: Vec<multicall3::Result>) -> Result<UniswapV3Pair> {
        let token0_returns = result
            .get(0)
            .ok_or_eyre("missing token0 call result")
            .and_then(|r| {
                if r.success {
                    abi::uniswap_v3_pool::IUniswapV3Pool::token0Call::abi_decode_returns(
                        &r.returnData,
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to decode token0 call returns")
                } else {
                    Err(eyre!("token0 call error"))
                }
            })?;

        let token1_returns = result
            .get(1)
            .ok_or_eyre("missing token1 call result")
            .and_then(|r| {
                if r.success {
                    abi::uniswap_v3_pool::IUniswapV3Pool::token1Call::abi_decode_returns(
                        &r.returnData,
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to decode token0 call returns")
                } else {
                    Err(eyre!("token1 call error"))
                }
            })?;

        let fee_returns = result
            .get(2)
            .ok_or_eyre("missing fee call result")
            .and_then(|r| {
                if r.success {
                    abi::uniswap_v3_pool::IUniswapV3Pool::feeCall::abi_decode_returns(
                        &r.returnData,
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to decode fee call returns")
                } else {
                    Err(eyre!("fee call error"))
                }
            })?;

        let factory_returns = result
            .get(3)
            .ok_or_eyre("missing factory call result")
            .and_then(|r| {
                if r.success {
                    abi::uniswap_v3_pool::IUniswapV3Pool::factoryCall::abi_decode_returns(
                        &r.returnData,
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to decode factory call returns")
                } else {
                    Err(eyre!("factory call error"))
                }
            })?;

        if factory_returns._0 != constants::UNISWAP_V3_FACTORY_ADDRESS {
            Err(eyre!("pair does not belong to UniswapV3 factory"))
        } else if token0_returns._0 != constants::WETH_ADDRESS
            && token1_returns._0 != constants::WETH_ADDRESS
        {
            Err(eyre!("pair does not contain weth"))
        } else {
            Ok(UniswapV3Pair::new(
                self.0,
                token0_returns._0,
                token1_returns._0,
                fee_returns._0,
            ))
        }
    }
}

impl From<Address> for UniswapV3PairInput {
    fn from(pair_address: Address) -> Self {
        Self(pair_address)
    }
}

impl UniswapV3PairInput {
    pub fn new(pair_address: Address) -> Self {
        Self(pair_address)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct UniswapV3Pair {
    address: Address,
    token0: Address,
    token1: Address,
    fee: u32,
}

impl UniswapV3Pair {
    pub fn new(address: Address, token0: Address, token1: Address, fee: u32) -> Self {
        Self {
            address,
            token0,
            token1,
            fee,
        }
    }

    fn open_eth_amount_in(&self) -> U256 {
        // TODO: use a static size for now, but should probably be dependent on
        // price impact
        constants::MAX_TRADE_SIZE_WEI
    }

    async fn quote_exact_input_single_call<T, P>(
        &self,
        params: abi::uniswap_v3_quoter_v2::IQuoterV2::QuoteExactInputSingleParams,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<abi::uniswap_v3_quoter_v2::IQuoterV2::quoteExactInputSingleReturn>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let tx_request = TransactionRequest::default()
            .with_from(rpc_provider.signer_address().clone())
            .with_to(Into::<TxKind>::into(
                constants::UNISWAP_V3_QUOTER_V2_ADDRESS,
            ))
            .with_input(
                abi::uniswap_v3_quoter_v2::IQuoterV2::quoteExactInputSingleCall { params }
                    .abi_encode()
                    .into(),
            );

        rpc_provider
            .inner()
            .call(&tx_request, Some(block_number.into()))
            .await
            .with_context(|| "quoteExactInputSingleCall failed")
            .and_then(|res| {
                abi::uniswap_v3_quoter_v2::IQuoterV2::quoteExactInputSingleCall::abi_decode_returns(
                    &res,
                    cfg!(debug_assertions),
                )
                .wrap_err("failed to decode quoteExactInputSingleCall returns")
            })
    }

    // returns (liquidity,  amount_out, sqrt_price_x96_after)
    async fn quote_exact_input_single_price_multicall<T, P>(
        &self,
        params: abi::uniswap_v3_quoter_v2::IQuoterV2::QuoteExactInputSingleParams,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<(U256, U256, U256)>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let multicall_tx_request = multicall_tx_request(vec![
            multicall3::Call3 {
                target: self.address,
                allowFailure: false,
                callData: abi::uniswap_v3_pool::IUniswapV3Pool::liquidityCall {}
                    .abi_encode()
                    .into(),
            },
            multicall3::Call3 {
                target: constants::UNISWAP_V3_QUOTER_V2_ADDRESS,
                allowFailure: false,
                callData: abi::uniswap_v3_quoter_v2::IQuoterV2::quoteExactInputSingleCall {
                    params: params,
                }
                .abi_encode()
                .into(),
            },
        ]);

        let multicall_results = rpc_provider
            .inner()
            .call(&multicall_tx_request, Some(block_number.into()))
            .await
            .with_context(|| "multicall failed")
            .and_then(|res| {
                multicall3::aggregate3Call::abi_decode_returns(&res, cfg!(debug_assertions))
                    .wrap_err("failed to decode multicall returns")
            })?;

        let liquidity = abi::uniswap_v3_pool::IUniswapV3Pool::liquidityCall::abi_decode_returns(
            &multicall_results.returnData[0].returnData,
            cfg!(debug_assertions),
        )
        .map(|res| U256::from(res._0))?;

        let (amount_out, sqrt_price_x96_after) =
            abi::uniswap_v3_quoter_v2::IQuoterV2::quoteExactInputSingleCall::abi_decode_returns(
                &multicall_results.returnData[1].returnData,
                cfg!(debug_assertions),
            )
            .map(|res| (res.amountOut, res.sqrtPriceX96After))?;

        Ok((liquidity, amount_out, sqrt_price_x96_after))
    }
}

impl DexPair<UniswapV3IndexedTrade> for UniswapV3Pair {
    fn token_address(&self) -> &Address {
        if self.token0 == constants::WETH_ADDRESS {
            &self.token1
        } else {
            &self.token0
        }
    }

    fn address(&self) -> &Address {
        &self.address
    }

    fn estimate_trade_gas(&self) -> U256 {
        uint!(130000_U256)
    }

    async fn simulate_trade_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<UniswapV3IndexedTrade>
    where
        T: Transport + Clone,
        P: Provider<T, alloy::network::Ethereum> + 'static,
    {
        let (amount0, amount1, sqrt_price_x96, liquidity) = match op {
            TradeRequestOp::Open => {
                let eth_amount_in = self.open_eth_amount_in();

                let (liquidity, amount_out, sqrt_price_x96) = self
                    .quote_exact_input_single_price_multicall(
                        abi::uniswap_v3_quoter_v2::IQuoterV2::QuoteExactInputSingleParams {
                            tokenIn: constants::WETH_ADDRESS,
                            tokenOut: self.token_address().clone(),
                            amountIn: eth_amount_in,
                            fee: self.fee,
                            sqrtPriceLimitX96: U256::ZERO,
                        },
                        block_number,
                        rpc_provider,
                    )
                    .await?;

                if self.token0 == constants::WETH_ADDRESS {
                    (
                        Signed::<256, 4>::from_raw(eth_amount_in),
                        Signed::<256, 4>::from_raw(amount_out).neg(),
                        sqrt_price_x96,
                        liquidity,
                    )
                } else {
                    (
                        Signed::<256, 4>::from_raw(amount_out).neg(),
                        Signed::<256, 4>::from_raw(eth_amount_in),
                        sqrt_price_x96,
                        liquidity,
                    )
                }
            }
            TradeRequestOp::Close {
                open_trade: IndexedTrade::UniswapV3(open_trade),
                ..
            } => {
                let open_trade_token_amount_out = if self.token0 == constants::WETH_ADDRESS {
                    let (_, amount1_value) = open_trade.amount1.into_sign_and_abs();
                    amount1_value
                } else {
                    let (_, amount0_value) = open_trade.amount0.into_sign_and_abs();
                    amount0_value
                };

                let (liquidity, amount_out, sqrt_price_x96) = self
                    .quote_exact_input_single_price_multicall(
                        abi::uniswap_v3_quoter_v2::IQuoterV2::QuoteExactInputSingleParams {
                            tokenIn: self.token_address().clone(),
                            tokenOut: constants::WETH_ADDRESS,
                            amountIn: open_trade_token_amount_out,
                            fee: self.fee,
                            sqrtPriceLimitX96: U256::ZERO,
                        },
                        block_number,
                        rpc_provider,
                    )
                    .await?;

                if self.token0 == constants::WETH_ADDRESS {
                    (
                        Signed::<256, 4>::from_raw(amount_out).neg(),
                        Signed::<256, 4>::from_raw(open_trade_token_amount_out),
                        sqrt_price_x96,
                        liquidity,
                    )
                } else {
                    (
                        Signed::<256, 4>::from_raw(open_trade_token_amount_out),
                        Signed::<256, 4>::from_raw(amount_out).neg(),
                        sqrt_price_x96,
                        liquidity,
                    )
                }
            }
            TradeRequestOp::Close { .. } => {
                eyre::bail!("invalid trade request op for uniswap v3 pair: {:?}", op)
            }
        };

        Ok(UniswapV3IndexedTrade::new(
            self.address,
            rpc_provider.signer_address().clone(),
            sqrt_price_x96,
            liquidity,
            amount0,
            amount1,
        ))
    }

    async fn trace_trade_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum>,
    {
        // FIXME: implement
        Ok(())
    }

    async fn make_trade_transaction_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        _block_timestamp: u64,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TransactionRequest>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        match op {
            TradeRequestOp::Open => {
                let eth_amount_in = self.open_eth_amount_in();
                let token_amount_out = self
                    .quote_exact_input_single_call(
                        abi::uniswap_v3_quoter_v2::IQuoterV2::QuoteExactInputSingleParams {
                            tokenIn: constants::WETH_ADDRESS,
                            tokenOut: self.token_address().clone(),
                            amountIn: eth_amount_in,
                            fee: self.fee,
                            sqrtPriceLimitX96: U256::ZERO,
                        },
                        block_number,
                        rpc_provider,
                    )
                    .await
                    .map(|res| res.amountOut)?;

                let tx_request = TransactionRequest::default()
                    .with_from(rpc_provider.signer_address().clone())
                    .with_to(Into::<TxKind>::into(
                        constants::UNISWAP_V3_ROUTER_02_ADDRESS,
                    ))
                    .with_input(
                        abi::uniswap_v3_swap_router::ISwapRouter::exactInputSingleCall {
                            params:
                                abi::uniswap_v3_swap_router::ISwapRouter::ExactInputSingleParams {
                                    tokenIn: constants::WETH_ADDRESS,
                                    tokenOut: self.token_address().clone(),
                                    fee: self.fee,
                                    recipient: rpc_provider.signer_address().clone(),
                                    amountIn: eth_amount_in,
                                    amountOutMinimum: token_amount_out,
                                    sqrtPriceLimitX96: U256::ZERO,
                                },
                        }
                        .abi_encode()
                        .into(),
                    );

                Ok(tx_request)
            }
            TradeRequestOp::Close {
                open_trade: IndexedTrade::UniswapV3(open_trade),
                ..
            } => {
                let open_trade_token_amount_out = if self.token0 == constants::WETH_ADDRESS {
                    let (_, amount1_value) = open_trade.amount1.into_sign_and_abs();
                    amount1_value
                } else {
                    let (_, amount0_value) = open_trade.amount0.into_sign_and_abs();
                    amount0_value
                };

                let eth_amount_out = self
                    .quote_exact_input_single_call(
                        abi::uniswap_v3_quoter_v2::IQuoterV2::QuoteExactInputSingleParams {
                            tokenOut: constants::WETH_ADDRESS,
                            tokenIn: self.token_address().clone(),
                            amountIn: open_trade_token_amount_out.clone().try_into().unwrap(),
                            fee: self.fee,
                            sqrtPriceLimitX96: U256::ZERO,
                        },
                        block_number,
                        rpc_provider,
                    )
                    .await
                    .map(|res| res.amountOut)?;

                let tx_request = TransactionRequest::default()
                    .with_from(rpc_provider.signer_address().clone())
                    .with_to(Into::<TxKind>::into(
                        constants::UNISWAP_V3_ROUTER_02_ADDRESS,
                    ))
                    .with_input(
                        abi::uniswap_v3_swap_router::ISwapRouter::exactInputSingleCall {
                            params:
                                abi::uniswap_v3_swap_router::ISwapRouter::ExactInputSingleParams {
                                    tokenOut: constants::WETH_ADDRESS,
                                    tokenIn: self.token_address().clone(),
                                    fee: self.fee,
                                    recipient: rpc_provider.signer_address().clone(),
                                    amountIn: open_trade_token_amount_out.try_into().unwrap(),
                                    amountOutMinimum: eth_amount_out,
                                    sqrtPriceLimitX96: U256::ZERO,
                                },
                        }
                        .abi_encode()
                        .into(),
                    );

                Ok(tx_request)
            }
            TradeRequestOp::Close { .. } => {
                eyre::bail!("invalid trade request op for uniswap v3 pair: {:?}", op)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{new_http_signer_provider, DexPair, TradeRequestOp, UniswapV3Pair};

    use alloy::primitives::address;

    use eyre::Result;
    use hex_literal::hex;

    #[tokio::test]
    async fn test_simulate_trade_request() -> Result<()> {
        let rpc_provider = new_http_signer_provider(
            url::Url::parse(
                "https://base-mainnet.g.alchemy.com/v2/GHF2kp-FpiiuNzmfpdP_dnms5WkewVQ-",
            )?,
            &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
            None,
            true,
        )
        .await?;
        let block_number = 14769718;
        let pair = UniswapV3Pair::new(
            address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA"),
            address!("4200000000000000000000000000000000000006"),
            address!("4ed4E862860beD51a9570b96d89aF5E1B0Efefed"),
            3000,
        );

        let open_trade = pair
            .simulate_trade_request(&TradeRequestOp::Open, block_number, &rpc_provider)
            .await?;

        // price_before from last swap in this block:
        // https://basescan.org/tx/0xfd133bb21dd2a5f14f2405e8bf2737eb1fbd6b9e5a98cba3707e279d3b65fe9f
        // FIXME
        /*
        assert_eq!(
            open_trade.price_before,
            UniswapV3IndexedTrade::parse_sqrt_price_x96(
                uint!(32808877825677231687005694057214_U256)
                    .try_into()
                    .unwrap()
            )
        );
        assert_eq!(
            open_trade.price_after,
            UniswapV3IndexedTrade::parse_sqrt_price_x96(
                uint!(32803281327708631759245388293764_U256)
                    .try_into()
                    .unwrap()
            )
        );
        assert_eq!(
            open_trade.abs_amount0,
            (*config::MAX_TRADE_SIZE_WEI).try_into().unwrap()
        );
        assert_eq!(
            open_trade.abs_amount1,
            uint!(170940376314918212446597_U256).try_into().unwrap()
        );
        assert_eq!(open_trade.maker, *rpc_provider.signer_address());
        assert_eq!(open_trade.pair_address, *pair.address());

        let open_abs_amount1 = open_trade.abs_amount1.clone();
        let close_trade = pair
            .simulate_trade_request(
                &TradeRequestOp::Close(open_trade.into()),
                block_number + 1,
                &rpc_provider,
            )
            .await?;

        assert_eq!(close_trade.abs_amount1, open_abs_amount1);
        */

        Ok(())
    }
}
