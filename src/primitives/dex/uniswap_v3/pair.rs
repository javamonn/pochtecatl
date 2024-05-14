use super::{
    super::{DexPair, DexPairInput},
    abi, UniswapV3IndexedTrade,
};

use crate::{abi::multicall3, config, providers::RpcProvider, trade_controller::TradeRequestOp};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockNumber, U256},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    sol_types::SolCall,
    transports::Transport,
};
use eyre::{eyre, OptionExt, Result, WrapErr};

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

        if token0_returns._0 == *config::WETH_ADDRESS || token1_returns._0 == *config::WETH_ADDRESS
        {
            Ok(UniswapV3Pair::new(
                self.0,
                token0_returns._0,
                token1_returns._0,
            ))
        } else {
            Err(eyre!("pair does not contain weth"))
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UniswapV3Pair {
    address: Address,
    token0: Address,
    token1: Address,
}

impl UniswapV3Pair {
    pub fn new(address: Address, token0: Address, token1: Address) -> Self {
        Self {
            address,
            token0,
            token1,
        }
    }
}

impl DexPair<UniswapV3IndexedTrade> for UniswapV3Pair {
    fn token_address(&self) -> &Address {
        if self.token0 == *config::WETH_ADDRESS {
            &self.token1
        } else {
            &self.token0
        }
    }

    fn address(&self) -> &Address {
        &self.address
    }

    fn estimate_trade_gas(&self) -> U256 {
        unimplemented!()
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
        unimplemented!()
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
        unimplemented!()
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
        P: Provider<T, Ethereum>,
    {
        unimplemented!()
    }
}
