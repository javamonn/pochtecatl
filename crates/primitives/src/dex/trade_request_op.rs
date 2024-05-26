use serde::{Deserialize, Serialize};

use alloy::primitives::TxHash;

use super::IndexedTrade;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeRequestOp {
    Open,
    Close {
        open_trade: IndexedTrade,
        open_trade_tx_hash: TxHash,
    },
}

impl TradeRequestOp {
    pub fn label(&self) -> String {
        match self {
            Self::Open => "Open".to_string(),
            Self::Close { .. } => "Close".to_string(),
        }
    }
}
