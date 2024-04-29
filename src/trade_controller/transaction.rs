use super::trade::TradeMetadata;

use crate::{indexer::ParseableTrade, rpc_provider::RpcProvider};

use alloy::{
    primitives::TxHash,
    providers::{PendingTransactionBuilder, Provider},
    rpc::types::eth::TransactionRequest,
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
    pub async fn send(tx_request: TransactionRequest, rpc_provider: &RpcProvider) -> Result<Self> {
        rpc_provider
            .send_transaction(tx_request)
            .await
            .map(|pending| Self(pending.tx_hash().clone()))
            .wrap_err("Failed to send_transaction")
    }

    pub async fn into_trade_metadata<T: ParseableTrade>(
        self,
        rpc_provider: &RpcProvider,
    ) -> Result<TradeMetadata<T>> {
        let tx_hash = self.0.clone();

        // Wait for the tx to confirm or reject
        let receipt = PendingTransactionBuilder::new(rpc_provider.inner().root(), self.0)
            .with_timeout(Some(Duration::from_secs(10)))
            .with_required_confirmations(1)
            .get_receipt()
            .await
            .wrap_err_with(|| format!("Failed to get receipt for tx hash {:?}", tx_hash))?;

        // Convert the committed tx into a trade
        TradeMetadata::from_receipt(&receipt, rpc_provider)
            .await
            .wrap_err_with(|| {
                format!(
                    "Failed to convert receipt to committed trade for tx hash {:?}",
                    tx_hash
                )
            })
    }
}

