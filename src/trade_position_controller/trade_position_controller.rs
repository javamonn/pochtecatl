use crate::{
    indexer::{IndexedUniswapV2Pair, UniswapV2PairTrade},
    rpc_provider::RpcProvider,
};

use alloy::{
    primitives::{Address, BlockNumber, U256},
    rpc::types::eth::{TransactionReceipt, TransactionRequest},
};

use eyre::{eyre, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};

pub struct CommittedTrade {
    block_number: BlockNumber,
    gas_fee: U256,
    trade: UniswapV2PairTrade,
}

pub struct CloseTradePosition {
    close: CommittedTrade,
    open: CommittedTrade,
}

pub enum TradePosition {
    PendingOpen,
    Open(CommittedTrade),
    PendingClose,
    Close {
        open: CommittedTrade,
        close: CommittedTrade,
    },
}

impl TradePosition {
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open(_))
    }
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

pub struct TradePositionController {
    rpc_provider: Arc<RpcProvider>,
    trade_positions: RwLock<FnvHashMap<Address, TradePosition>>,
}

impl TradePositionController {
    pub fn new(rpc_provider: Arc<RpcProvider>) -> Self {
        Self {
            rpc_provider,
            trade_positions: RwLock::new(FnvHashMap::default()),
        }
    }

    pub fn trade_positions(&self) -> &RwLock<FnvHashMap<Address, TradePosition>> {
        &self.trade_positions
    }

    pub async fn close_position(
        &self,
        tx_request: TransactionRequest,
        pair_address: Address,
    ) -> Result<()> {
        unimplemented!()
    }

    pub async fn uniswap_v2_position_is_valid(
        &self,
        pair: &IndexedUniswapV2Pair,
        tx_request: &TransactionRequest,
    ) -> Result<bool> {
        unimplemented!()
    }

    pub async fn open_position(
        &self,
        tx_request: TransactionRequest,
        pair_address: Address,
    ) -> Result<()> {
        // ensure that we do not already have a position for this pair and add the position to
        // the store
        {
            let mut trade_positions = self.trade_positions.write().unwrap();
            if trade_positions.contains_key(&pair_address) {
                return Err(eyre!("Position already exists for pair {}", pair_address));
            } else {
                trade_positions.insert(pair_address, TradePosition::PendingOpen);
            }
        }

        match self
            .rpc_provider
            .send_transaction(tx_request)
            .await
            .and_then(|receipt| (&receipt).try_into())
        {
            Ok(committed_trade) => {
                // Add the committed position to the store
                {
                    let mut trade_positions = self.trade_positions.write().unwrap();
                    trade_positions.insert(pair_address, TradePosition::Open(committed_trade));
                }

                Ok(())
            }
            Err(err) => {
                // Remove the pending position from the store
                {
                    let mut trade_positions = self.trade_positions.write().unwrap();
                    trade_positions.remove(&pair_address);
                }

                Err(err)
            }
        }
    }
}
