use crate::{primitives::UniswapV2PairTrade, providers::RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::{BlockNumber, U256},
    providers::Provider,
    rpc::types::eth::TransactionReceipt,
    transports::Transport,
};

use eyre::{eyre, OptionExt, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ParsedTrade {
    UniswapV2PairTrade(UniswapV2PairTrade),
}

impl TryFrom<&TransactionReceipt> for ParsedTrade {
    type Error = eyre::Error;

    fn try_from(tx_receipt: &TransactionReceipt) -> Result<Self> {
        let receipt = tx_receipt
            .as_ref()
            .as_receipt()
            .ok_or_eyre("Failed to convert TransactionReceipt to Receipt")?;

        for (idx, log) in receipt.logs.iter().enumerate() {
            if let Some(parsed_trade) =
                UniswapV2PairTrade::try_parse_from_log(log, &receipt.logs, idx)
            {
                return Ok(ParsedTrade::UniswapV2PairTrade(parsed_trade));
            }
        }

        Err(eyre!("No parsed trade found"))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TradeMetadata {
    block_number: BlockNumber,
    block_timestamp: u64,
    gas_fee: U256,
    parsed_trade: ParsedTrade,
}

impl TradeMetadata {
    pub fn new(
        block_number: BlockNumber,
        block_timestamp: u64,
        gas_fee: U256,
        parsed_trade: ParsedTrade,
    ) -> Self {
        Self {
            block_number,
            block_timestamp,
            gas_fee,
            parsed_trade,
        }
    }

    pub fn block_timestamp(&self) -> &u64 {
        &self.block_timestamp
    }

    pub fn parsed_trade(&self) -> &ParsedTrade {
        &self.parsed_trade
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
        let parsed_trade = receipt.try_into()?;

        Ok(Self {
            block_number,
            block_timestamp,
            gas_fee,
            parsed_trade,
        })
    }
}
