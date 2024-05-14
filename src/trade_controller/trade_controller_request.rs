use super::TradeMetadata;
use crate::{
    primitives::{IndexedTrade, Pair},
    providers::RpcProvider,
};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockNumber},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    transports::Transport,
};

use eyre::Result;

pub trait TradeControllerRequest {
    fn pair_address(&self) -> &Address;

    async fn trace<T, P>(&self, rpc_provider: &RpcProvider<T, P>) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;

    fn simulate_trade_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> impl std::future::Future<Output = Result<TradeMetadata>> + Send
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;

    fn make_trade_transaction_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> impl std::future::Future<Output = Result<TransactionRequest>> + Send
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;
}

#[derive(Debug)]
pub enum TradeRequestOp {
    Open,
    Close(IndexedTrade),
}

#[derive(Debug)]
pub struct TradeRequest {
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub op: TradeRequestOp,
    pub pair: Pair,
}

impl TradeRequest {
    pub fn open(block_number: BlockNumber, block_timestamp: u64, pair: Pair) -> Self {
        Self {
            block_number,
            block_timestamp,
            pair,
            op: TradeRequestOp::Open,
        }
    }

    pub fn close(
        block_number: BlockNumber,
        block_timestamp: u64,
        pair: Pair,
        open_trade: IndexedTrade,
    ) -> Self {
        Self {
            block_number,
            block_timestamp,
            pair,
            op: TradeRequestOp::Close(open_trade),
        }
    }
}

impl TradeControllerRequest for TradeRequest {
    fn pair_address(&self) -> &Address {
        self.pair.address()
    }

    async fn trace<T, P>(&self, rpc_provider: &RpcProvider<T, P>) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        self.pair
            .trace_trade_request(&self.op, self.block_number, rpc_provider)
            .await
    }

    async fn simulate_trade_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TradeMetadata>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let indexed_trade = self
            .pair
            .simulate_trade_request(&self.op, self.block_number, rpc_provider)
            .await?;

        TradeMetadata::from_simulated_indexed_trade(indexed_trade, self, rpc_provider).await
    }

    async fn make_trade_transaction_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TransactionRequest>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        self.pair
            .make_trade_transaction_request(
                &self.op,
                self.block_number,
                self.block_timestamp,
                rpc_provider,
            )
            .await
    }
}
