use crate::{indexer::ParseableTrade, rpc_provider::RpcProvider};

use alloy::{
    primitives::{BlockNumber, U256},
    rpc::types::eth::TransactionReceipt,
};

use eyre::{eyre, Result};

pub struct TradeMetadata<T: ParseableTrade> {
    block_number: BlockNumber,
    block_timestamp: u64,
    gas_fee: U256,
    parsed_trade: T,
}

impl<T: ParseableTrade> TradeMetadata<T> {
    pub async fn from_receipt(
        receipt: &TransactionReceipt,
        rpc_provider: &RpcProvider,
    ) -> Result<Self> {
        let block_number = receipt
            .block_number
            .ok_or_else(|| eyre!("Block number not found"))?;
        let gas_fee = U256::from(
            receipt
                .gas_used
                .ok_or_else(|| eyre!("Gas used not found"))?,
        ) * U256::from(receipt.effective_gas_price);
        let parsed_trade = receipt
            .as_ref()
            .as_receipt()
            .ok_or_else(|| eyre!("Transaction receipt is not a receipt"))
            .and_then(|r| {
                for (idx, log) in r.logs.iter().enumerate() {
                    if let Some(parsed_trade) = ParseableTrade::parse_from_log(log, &r.logs, idx) {
                        return Ok(parsed_trade);
                    }
                }

                Err(eyre!("No Uniswap V2 pair trade found in receipt"))
            })?;
        let block_timestamp = rpc_provider
            .get_block_header(block_number)
            .await
            .and_then(|header| {
                header.ok_or_else(|| eyre!("block header {:?} not found", receipt.block_number))
            })
            .map(|header| header.timestamp.to::<u64>())?;

        Ok(Self {
            block_number,
            block_timestamp,
            gas_fee,
            parsed_trade,
        })
    }

    pub fn block_timestamp(&self) -> &u64 {
        &self.block_timestamp
    }
}

pub enum Trade<T: ParseableTrade> {
    PendingOpen,
    Open(TradeMetadata<T>),
    PendingClose,
    Closed(TradeMetadata<T>, TradeMetadata<T>),
}
