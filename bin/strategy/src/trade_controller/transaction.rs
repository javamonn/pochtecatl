use pochtecatl_primitives::{IndexedTrade, RpcProvider, TradeMetadata, TradeRequestOp};

use alloy::{
    network::Ethereum,
    primitives::{TxHash, U256, Address},
    providers::{PendingTransactionBuilder, Provider},
    rpc::types::eth::TransactionRequest,
    transports::Transport,
};

use eyre::{eyre, Result, WrapErr};
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
        op: TradeRequestOp,
        token_address: Address,
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

        let block_number = confirmed_receipt
            .block_number
            .ok_or_else(|| eyre!("Block number not found"))?;
        let gas_fee = U256::from(
            confirmed_receipt
                .gas_used
                .ok_or_else(|| eyre!("Gas used not found"))?,
        ) * U256::from(confirmed_receipt.effective_gas_price);
        let block_timestamp = rpc_provider
            .block_provider()
            .get_block_header(block_number)
            .await
            .and_then(|header| {
                header.ok_or_else(|| {
                    eyre!(
                        "block header {:?} not found",
                        confirmed_receipt.block_number
                    )
                })
            })
            .map(|header| header.timestamp.to::<u64>())?;

        let indexed_trade = {
            let tx_hash = confirmed_receipt.transaction_hash;
            IndexedTrade::from_receipt(&confirmed_receipt)
                .first()
                .cloned()
                .ok_or_else(|| eyre!("No indexed trade found in receipt {:?}", tx_hash))?
        };

        Ok(TradeMetadata::new(
            confirmed_receipt.transaction_hash,
            block_number,
            block_timestamp,
            op,
            token_address,
            gas_fee,
            indexed_trade,
        ))
    }
}
