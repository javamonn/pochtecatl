use super::TradeMetadata;

use crate::{indexer::ParseableTrade, rpc_provider::RpcProvider};

use alloy::{primitives::Address, rpc::types::eth::TransactionRequest};

use eyre::Result;

pub trait TradeRequest<P: ParseableTrade> {
    fn as_transaction_request(&self, signer_address: Address) -> TransactionRequest;
    fn address(&self) -> &Address;

    async fn trace(&self, rpc_provider: &RpcProvider) -> Result<()>;
    async fn as_backtest_trade_metadata(
        &self,
        rpc_provider: &RpcProvider,
    ) -> Result<TradeMetadata<P>>;
}
