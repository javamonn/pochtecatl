use alloy::primitives::U256;

#[derive(Debug)]
pub enum TradeRequestIntent {
    Open { eth_amount_in: U256 },
    Close { token_amount_in: U256 },
}

