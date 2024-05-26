use super::{IndexedTrade, TradeRequestOp};

use alloy::primitives::{Address, BlockNumber, TxHash, U256};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeMetadata {
    tx_hash: TxHash,
    block_number: BlockNumber,
    block_timestamp: u64,
    op: TradeRequestOp,
    token_address: Address,
    gas_fee: U256,
    indexed_trade: IndexedTrade,
}

impl TradeMetadata {
    pub fn new(
        tx_hash: TxHash,
        block_number: BlockNumber,
        block_timestamp: u64,
        op: TradeRequestOp,
        token_address: Address,
        gas_fee: U256,
        indexed_trade: IndexedTrade,
    ) -> Self {
        Self {
            tx_hash,
            block_number,
            block_timestamp,
            op,
            token_address,
            gas_fee,
            indexed_trade,
        }
    }

    pub fn tx_hash(&self) -> &TxHash {
        &self.tx_hash
    }

    pub fn block_timestamp(&self) -> &u64 {
        &self.block_timestamp
    }

    pub fn indexed_trade(&self) -> &IndexedTrade {
        &self.indexed_trade
    }

    pub fn op(&self) -> &TradeRequestOp {
        &self.op
    }

    pub fn token_address(&self) -> &Address {
        &self.token_address
    }

    pub fn block_number(&self) -> &BlockNumber {
        &self.block_number
    }

    pub fn gas_fee(&self) -> &U256 {
        &self.gas_fee
    }
}
