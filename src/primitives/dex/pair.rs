use super::{IndexedTrade, UniswapV2Pair, UniswapV2PairInput, UniswapV3Pair, UniswapV3PairInput};

use crate::{abi::multicall3, providers::RpcProvider, trade_controller::TradeRequestOp};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockNumber, U256},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    transports::Transport,
};

use eyre::Result;

pub trait DexPairInput<P>
where
    P: Into<Pair>,
{
    fn address(&self) -> &Address;
    fn calls(&self) -> Vec<multicall3::Call3>;
    fn decode(&self, result: Vec<multicall3::Result>) -> Result<P>;
}

pub enum PairInput {
    UniswapV2(UniswapV2PairInput),
    UniswapV3(UniswapV3PairInput),
}
impl PairInput {
    pub fn address(&self) -> &Address {
        match self {
            Self::UniswapV2(pair_input) => pair_input.address(),
            Self::UniswapV3(pair_input) => pair_input.address(),
        }
    }

    pub fn calls(&self) -> Vec<multicall3::Call3> {
        match self {
            Self::UniswapV2(pair_input) => pair_input.calls(),
            Self::UniswapV3(pair_input) => pair_input.calls(),
        }
    }

    pub fn decode(&self, result: Vec<multicall3::Result>) -> Result<Pair> {
        match self {
            Self::UniswapV2(pair_input) => pair_input.decode(result).map(Into::into),
            Self::UniswapV3(pair_input) => pair_input.decode(result).map(Into::into),
        }
    }
}

impl From<&IndexedTrade> for PairInput {
    fn from(indexed_trade: &IndexedTrade) -> Self {
        let pair_address = indexed_trade.pair_address().clone();
        match indexed_trade {
            IndexedTrade::UniswapV2(indexed_trade) => Self::UniswapV2(pair_address.into()),
            IndexedTrade::UniswapV3(indexed_trade) => Self::UniswapV3(pair_address.into()),
        }
    }
}

pub trait DexPair<I>
where
    I: Into<IndexedTrade>,
{
    fn address(&self) -> &Address;
    fn token_address(&self) -> &Address;
    fn estimate_trade_gas(&self) -> U256;

    async fn simulate_trade_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<I>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;

    async fn trace_trade_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;

    async fn make_trade_transaction_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        block_timestamp: u64,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TransactionRequest>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pair {
    UniswapV2(UniswapV2Pair),
    UniswapV3(UniswapV3Pair),
}

impl Pair {
    pub fn address(&self) -> &Address {
        match self {
            Self::UniswapV2(pair) => pair.address(),
            Self::UniswapV3(pair) => pair.address(),
        }
    }

    pub fn token_address(&self) -> &Address {
        match self {
            Self::UniswapV2(pair) => pair.token_address(),
            Self::UniswapV3(pair) => pair.token_address(),
        }
    }

    pub fn estimate_trade_gas(&self) -> U256 {
        match self {
            Self::UniswapV2(pair) => pair.estimate_trade_gas(),
            Self::UniswapV3(pair) => pair.estimate_trade_gas(),
        }
    }

    pub async fn simulate_trade_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<IndexedTrade>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        match self {
            Self::UniswapV2(pair) => pair
                .simulate_trade_request(op, block_number, rpc_provider)
                .await
                .map(Into::into),
            Self::UniswapV3(pair) => pair
                .simulate_trade_request(op, block_number, rpc_provider)
                .await
                .map(Into::into),
        }
    }

    pub async fn trace_trade_request<T, P>(
        &self,
        op: &TradeRequestOp,
        block_number: BlockNumber,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        match self {
            Self::UniswapV2(pair) => {
                pair.trace_trade_request(op, block_number, rpc_provider)
                    .await
            }
            Self::UniswapV3(pair) => {
                pair.trace_trade_request(op, block_number, rpc_provider)
                    .await
            }
        }
    }

    pub async fn make_trade_transaction_request<T, P>(
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
        match self {
            Self::UniswapV2(pair) => {
                pair.make_trade_transaction_request(op, block_number, block_timestamp, rpc_provider)
                    .await
            }
            Self::UniswapV3(pair) => {
                pair.make_trade_transaction_request(op, block_number, block_timestamp, rpc_provider)
                    .await
            }
        }
    }
}

impl From<UniswapV2Pair> for Pair {
    fn from(pair: UniswapV2Pair) -> Self {
        Self::UniswapV2(pair)
    }
}

impl From<UniswapV3Pair> for Pair {
    fn from(pair: UniswapV3Pair) -> Self {
        Self::UniswapV3(pair)
    }
}
