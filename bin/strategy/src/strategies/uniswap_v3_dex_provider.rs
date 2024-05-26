use crate::{primitives::UniswapV3PairMessage, trade_controller::TradeRequest};

use alloy::primitives::{BlockNumber, U256};

pub fn make_open_trade_request(
    pair: &UniswapV3PairMessage,
    block_number: BlockNumber,
    block_timestamp: u64,
) -> TradeRequest {
    unimplemented!()
}

