use super::TradeMetadata;

use crate::{indexer::ParseableTrade, providers::RpcProvider};

use alloy::{
    network::Ethereum, primitives::Address, providers::Provider,
    rpc::types::eth::TransactionRequest, transports::Transport,
};

use eyre::Result;

pub trait TradeRequest<PT, T, P>: Send + Sync + 'static
where
    PT: ParseableTrade,
    T: Transport + Clone,
    P: Provider<T, Ethereum>,
{
    fn as_transaction_request(&self, signer_address: Address) -> TransactionRequest;
    fn address(&self) -> &Address;

    async fn trace(&self, rpc_provider: &RpcProvider<T, P>) -> Result<()>;

    fn as_backtest_trade_metadata(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> impl std::future::Future<Output = Result<TradeMetadata<PT>>> + Send;
}
