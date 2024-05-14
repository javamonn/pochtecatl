use super::TradeMetadata;

use crate::providers::RpcProvider;

use alloy::{
    network::Ethereum,
    primitives::TxHash,
    providers::{PendingTransactionBuilder, Provider},
    rpc::types::eth::TransactionRequest,
    transports::Transport,
};

use eyre::{Result, WrapErr};
use std::{fmt::Display, time::Duration};

#[derive(Debug)]
pub struct Transaction(TxHash);
impl Display for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Transaction({})", self.0)
    }
}
impl Transaction {
    pub async fn send<T, P>(
        tx_request: TransactionRequest,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<Self>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        rpc_provider
            .send_transaction(tx_request)
            .await
            .map(|pending| Self(pending.tx_hash().clone()))
            .wrap_err("Failed to send_transaction")
    }

    pub async fn into_trade_metadata<T, P>(
        self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TradeMetadata>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let tx_hash = self.0.clone();

        // Wait for the tx to confirm or reject
        let confirmed_receipt = PendingTransactionBuilder::new(rpc_provider.inner().root(), self.0)
            .with_timeout(Some(Duration::from_secs(10)))
            .with_required_confirmations(1)
            .get_receipt()
            .await
            .wrap_err_with(|| format!("Failed to get receipt for tx hash {:?}", tx_hash))?;

        TradeMetadata::from_receipt(&confirmed_receipt, rpc_provider).await
    }
}
