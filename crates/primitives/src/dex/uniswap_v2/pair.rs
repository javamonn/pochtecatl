use super::{
    super::{DexPair, DexPairInput, IndexedTrade, TradeRequestOp},
    abi, UniswapV2IndexedTrade,
};
use crate::{abi::multicall3, constants, RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::{uint, Address, BlockNumber, U256},
    providers::Provider,
    rpc::types::eth::{BlockId, TransactionRequest},
    sol_types::SolCall,
    transports::Transport,
};

use eyre::{eyre, OptionExt, Result, WrapErr};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct UniswapV2PairInput(pub Address);

impl DexPairInput<UniswapV2Pair> for UniswapV2PairInput {
    fn address(&self) -> &Address {
        &self.0
    }

    fn calls(&self) -> Vec<multicall3::Call3> {
        vec![
            multicall3::Call3 {
                target: self.0,
                allowFailure: true,
                callData: abi::uniswap_v2_pair::IUniswapV2Pair::token0Call {}
                    .abi_encode()
                    .into(),
            },
            multicall3::Call3 {
                target: self.0,
                allowFailure: true,
                callData: abi::uniswap_v2_pair::IUniswapV2Pair::token1Call {}
                    .abi_encode()
                    .into(),
            },
        ]
    }

    fn decode(&self, result: Vec<multicall3::Result>) -> Result<UniswapV2Pair> {
        let token0_returns = result
            .get(0)
            .ok_or_eyre("missing token0 call result")
            .and_then(|r| {
                if r.success {
                    abi::uniswap_v2_pair::IUniswapV2Pair::token0Call::abi_decode_returns(
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
                    abi::uniswap_v2_pair::IUniswapV2Pair::token1Call::abi_decode_returns(
                        &r.returnData,
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to decode token0 call returns")
                } else {
                    Err(eyre!("token1 call error"))
                }
            })?;

        if token0_returns._0 == constants::WETH_ADDRESS
            || token1_returns._0 == constants::WETH_ADDRESS
        {
            Ok(UniswapV2Pair::new(
                self.0,
                token0_returns._0,
                token1_returns._0,
            ))
        } else {
            Err(eyre!("pair does not contain weth"))
        }
    }
}

impl From<Address> for UniswapV2PairInput {
    fn from(pair_address: Address) -> Self {
        Self(pair_address)
    }
}

impl UniswapV2PairInput {
    pub fn new(pair_address: Address) -> Self {
        Self(pair_address)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UniswapV2Pair {
    address: Address,
    token0: Address,
    token1: Address,
}

const BP_FACTOR: U256 = uint!(10000_U256);
const MAX_TRADE_SIZE_PRICE_IMPACT_BP: U256 = uint!(50_U256);

fn get_eth_amount_in(weth_reserve: U256) -> U256 {
    let max_for_price_impact = (MAX_TRADE_SIZE_PRICE_IMPACT_BP * weth_reserve) / BP_FACTOR;
    if max_for_price_impact < constants::MAX_TRADE_SIZE_WEI {
        max_for_price_impact
    } else {
        constants::MAX_TRADE_SIZE_WEI
    }
}

impl UniswapV2Pair {
    pub fn new(address: Address, token0: Address, token1: Address) -> Self {
        Self {
            address,
            token0,
            token1,
        }
    }

    // returns (token, weth) reserves
    async fn get_reserves<T, P>(
        &self,
        block_id: Option<BlockId>,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<(U256, U256)>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let result = rpc_provider
            .inner()
            .call(
                &abi::uniswap_v2_pair::get_reserves_tx_request(
                    *rpc_provider.signer_address(),
                    self.address,
                ),
                block_id,
            )
            .await
            .with_context(|| format!("failed to get reserves for pair {}", self.address))
            .and_then(|res| {
                abi::uniswap_v2_pair::IUniswapV2Pair::getReservesCall::abi_decode_returns(
                    res.as_ref(),
                    cfg!(debug_assertions),
                )
                .with_context(|| format!("failed to decode reserves for pair {}", self.address))
            })?;

        if self.token0 == constants::WETH_ADDRESS {
            Ok((U256::from(result.reserve1), U256::from(result.reserve0)))
        } else {
            Ok((U256::from(result.reserve0), U256::from(result.reserve1)))
        }
    }
}

impl DexPair<UniswapV2IndexedTrade> for UniswapV2Pair {
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
    ) -> Result<UniswapV2IndexedTrade>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let (token_reserve, weth_reserve) = self
            .get_reserves(Some(block_number.into()), rpc_provider)
            .await?;
        let (amount0_in, amount1_in, amount0_out, amount1_out, reserve0, reserve1) = match op {
            TradeRequestOp::Open => {
                let eth_amount_in = get_eth_amount_in(weth_reserve);
                let output_token_amount_min = abi::uniswap_v2_router::get_amount_out(
                    eth_amount_in,
                    weth_reserve,
                    token_reserve,
                );
                if self.token0 == constants::WETH_ADDRESS {
                    (
                        eth_amount_in,
                        U256::ZERO,
                        U256::ZERO,
                        output_token_amount_min,
                        weth_reserve + eth_amount_in,
                        token_reserve - output_token_amount_min,
                    )
                } else {
                    (
                        U256::ZERO,
                        eth_amount_in,
                        output_token_amount_min,
                        U256::ZERO,
                        token_reserve - output_token_amount_min,
                        weth_reserve + eth_amount_in,
                    )
                }
            }
            TradeRequestOp::Close {
                open_trade: IndexedTrade::UniswapV2(trade),
                ..
            } => {
                if self.token0 == constants::WETH_ADDRESS {
                    let open_trade_token_amount_out = trade.amount1_out;
                    let min_eth_amount_out = abi::uniswap_v2_router::get_amount_out(
                        open_trade_token_amount_out, // token amount received from trade
                        token_reserve,
                        weth_reserve,
                    );

                    (
                        U256::ZERO,
                        open_trade_token_amount_out,
                        min_eth_amount_out,
                        U256::ZERO,
                        weth_reserve - min_eth_amount_out,
                        token_reserve + open_trade_token_amount_out,
                    )
                } else {
                    let open_trade_token_amount_out = trade.amount0_out;
                    let close_min_eth_amount_out = abi::uniswap_v2_router::get_amount_out(
                        open_trade_token_amount_out,
                        token_reserve,
                        weth_reserve,
                    );

                    (
                        open_trade_token_amount_out,
                        U256::ZERO,
                        U256::ZERO,
                        close_min_eth_amount_out,
                        token_reserve + open_trade_token_amount_out,
                        weth_reserve - close_min_eth_amount_out,
                    )
                }
            }
            TradeRequestOp::Close { .. } => {
                eyre::bail!("invalid trade request op for uniswap v2 pair: {:?}", op)
            }
        };

        Ok(UniswapV2IndexedTrade::new(
            self.address,
            amount0_in,
            amount1_in,
            amount0_out,
            amount1_out,
            reserve0,
            reserve1,
            *rpc_provider.signer_address(),
        ))
    }

    async fn make_trade_transaction_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        block_timestamp: u64,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TransactionRequest>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let (token_reserve, weth_reserve) = self
            .get_reserves(Some(block_number.into()), rpc_provider)
            .await?;

        match op {
            TradeRequestOp::Open => {
                let eth_amount_in = get_eth_amount_in(weth_reserve);
                let output_token_amount_min = abi::uniswap_v2_router::get_amount_out(
                    eth_amount_in,
                    weth_reserve,
                    token_reserve,
                );
                Ok(
                    abi::uniswap_v2_router::swap_exact_eth_for_tokens_tx_request(
                        *rpc_provider.signer_address(),
                        eth_amount_in,
                        output_token_amount_min,
                        *self.token_address(),
                        U256::from(block_timestamp + (constants::AVERAGE_BLOCK_TIME_SECONDS * 2)),
                    ),
                )
            }
            TradeRequestOp::Close {
                open_trade: IndexedTrade::UniswapV2(trade),
                ..
            } => {
                let open_trade_token_amount_out = if self.token0 == constants::WETH_ADDRESS {
                    trade.amount1_out
                } else {
                    trade.amount0_out
                };
                let min_eth_amount_out = abi::uniswap_v2_router::get_amount_out(
                    open_trade_token_amount_out,
                    token_reserve,
                    weth_reserve,
                );

                Ok(
                    abi::uniswap_v2_router::swap_exact_tokens_for_eth_tx_request(
                        *rpc_provider.signer_address(),
                        open_trade_token_amount_out,
                        min_eth_amount_out,
                        *self.token_address(),
                        U256::from(block_timestamp + (constants::AVERAGE_BLOCK_TIME_SECONDS * 2)),
                    ),
                )
            }
            TradeRequestOp::Close { .. } => Err(eyre::eyre!(
                "invalid trade request op for uniswap v2 pair: {:?}",
                op
            )),
        }
    }

    async fn trace_trade_request<T, P>(
        &self,
        _op: &TradeRequestOp,
        _block_number: BlockNumber,
        _rpc_provider: &RpcProvider<T, P>,
    ) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum>,
    {
        // FIXME: implement
        Ok(())
    }
}
