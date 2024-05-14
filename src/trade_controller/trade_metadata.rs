use crate::{primitives::IndexedTrade, providers::RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::{BlockNumber, U256},
    providers::Provider,
    rpc::types::eth::{Block, BlockTransactions, TransactionReceipt},
    transports::Transport,
};

use eyre::{eyre, OptionExt, Result};
use serde::{Deserialize, Serialize};

use super::{TradeControllerRequest, TradeRequest};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeMetadata {
    block_number: BlockNumber,
    block_timestamp: u64,
    gas_fee: U256,
    indexed_trade: IndexedTrade,
}

impl TradeMetadata {
    pub fn new(
        block_number: BlockNumber,
        block_timestamp: u64,
        gas_fee: U256,
        indexed_trade: IndexedTrade,
    ) -> Self {
        Self {
            block_number,
            block_timestamp,
            gas_fee,
            indexed_trade,
        }
    }

    pub fn block_timestamp(&self) -> &u64 {
        &self.block_timestamp
    }

    pub fn indexed_trade(&self) -> &IndexedTrade {
        &self.indexed_trade
    }

    pub fn block_number(&self) -> &BlockNumber {
        &self.block_number
    }

    pub async fn from_receipt<T, P>(
        receipt: &TransactionReceipt,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<Self>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let block_number = receipt
            .block_number
            .ok_or_else(|| eyre!("Block number not found"))?;
        let gas_fee = U256::from(
            receipt
                .gas_used
                .ok_or_else(|| eyre!("Gas used not found"))?,
        ) * U256::from(receipt.effective_gas_price);
        let block_timestamp = rpc_provider
            .block_provider()
            .get_block_header(block_number)
            .await
            .and_then(|header| {
                header.ok_or_else(|| eyre!("block header {:?} not found", receipt.block_number))
            })
            .map(|header| header.timestamp.to::<u64>())?;

        let indexed_trade = {
            let tx_hash = receipt.transaction_hash;
            IndexedTrade::from_receipt(receipt)
                .first()
                .cloned()
                .ok_or_else(|| eyre!("No indexed trade found in receipt {:?}", tx_hash))?
        };

        Ok(Self {
            block_number,
            block_timestamp,
            gas_fee,
            indexed_trade,
        })
    }

    pub async fn from_simulated_indexed_trade<T, P> (
        indexed_trade: IndexedTrade,
        request: &TradeRequest,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<Self>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        if !*crate::config::IS_BACKTEST {
            return Err(eyre!("Only avaialble in backtest"));
        }

        // Gets the effective_gas_price of the tx in the middle of the block we would've
        // confirmed in while backtesting, and multiples it by an estimate of the gas used
        // by the trade to get the estimated gas fee.
        let estimated_gas_fee = {
            let median_tx_hash = rpc_provider
                .block_provider()
                .get_block(request.block_number)
                .await
                .ok()
                .and_then(|block| match block {
                    Some(Block {
                        transactions: BlockTransactions::Hashes(hashes),
                        ..
                    }) => hashes.get(hashes.len() / 2).cloned(),
                    _ => None,
                })
                .ok_or_eyre("Failed to get median tx hash")?;

            let median_tx_effictive_gas_price = rpc_provider
                .get_transaction_receipt(median_tx_hash)
                .await
                .ok()
                .and_then(|receipt| receipt.map(|receipt| receipt.effective_gas_price))
                .ok_or_eyre("Failed to get median tx receipt")?;

            U256::from(median_tx_effictive_gas_price) * request.pair.estimate_trade_gas()
        };

        Ok(Self {
            block_number: request.block_number,
            block_timestamp: request.block_timestamp,
            gas_fee: estimated_gas_fee,
            indexed_trade,
        })
    }
}
