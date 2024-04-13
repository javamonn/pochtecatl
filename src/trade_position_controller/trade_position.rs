use crate::indexer::UniswapV2PairTrade;

use alloy::{
    primitives::{BlockNumber, U256},
    rpc::types::eth::TransactionReceipt,
};

use eyre::eyre;

pub struct CommittedTrade {
    block_number: BlockNumber,
    gas_fee: U256,
    trade: UniswapV2PairTrade,
}

impl TryFrom<&TransactionReceipt> for CommittedTrade {
    type Error = eyre::Error;

    fn try_from(receipt: &TransactionReceipt) -> std::prelude::v1::Result<Self, Self::Error> {
        let block_number = receipt
            .block_number
            .ok_or_else(|| eyre!("Block number not found"))?;
        let gas_fee = U256::from(
            receipt
                .gas_used
                .ok_or_else(|| eyre!("Gas used not found"))?,
        ) * U256::from(receipt.effective_gas_price);
        let trade = receipt
            .as_ref()
            .as_receipt()
            .ok_or_else(|| eyre!("Transaction receipt is not a receipt"))
            .and_then(|r| {
                for (idx, log) in r.logs.iter().enumerate() {
                    if let Some(uniswap_v2_pair_trade) =
                        UniswapV2PairTrade::parse(log, &r.logs, idx)
                    {
                        return Ok(uniswap_v2_pair_trade);
                    }
                }

                Err(eyre!("No Uniswap V2 pair trade found in receipt"))
            })?;

        Ok(Self {
            block_number,
            gas_fee,
            trade,
        })
    }
}



pub struct ClosedTradePosition {
    close: CommittedTrade,
    open: CommittedTrade,
}

pub enum TradePosition {
    PendingOpen,
    Open(CommittedTrade),
    PendingClose,
    Closed(ClosedTradePosition),
}

impl TradePosition {
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open(_))
    }
}

