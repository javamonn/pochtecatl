use crate::{
    config,
    indexer::IndexedUniswapV2Pair,
    trade_controller::{
        ParsedTrade, TradeMetadata, TradeRequest, TradeRequestIntent, UniswapV2TradeRequest,
    },
};

use alloy::primitives::{uint, BlockNumber, U256};

use eyre::Result;

const BP_FACTOR: U256 = uint!(10000_U256);
const MAX_TRADE_SIZE_PRICE_IMPACT_BP: U256 = uint!(50_U256);
const MAX_TRADE_SIZE_WEI: U256 = uint!(1000000000000000000_U256);

pub fn make_open_trade_request(
    pair: &IndexedUniswapV2Pair,
    block_number: BlockNumber,
    block_timestamp: u64,
) -> TradeRequest {
    let eth_amount_in = {
        let max_for_price_impact = (MAX_TRADE_SIZE_PRICE_IMPACT_BP * pair.weth_reserve) / BP_FACTOR;
        if max_for_price_impact < MAX_TRADE_SIZE_WEI {
            max_for_price_impact
        } else {
            MAX_TRADE_SIZE_WEI
        }
    };

    TradeRequest::UniswapV2(UniswapV2TradeRequest::new(
        pair.pair_address,
        pair.token_address,
        block_number,
        block_timestamp,
        pair.weth_reserve,
        pair.token_reserve,
        TradeRequestIntent::Open { eth_amount_in },
    ))
}

pub fn make_close_trade_request(
    open_trade_metadata: &TradeMetadata,
    pair: &IndexedUniswapV2Pair,
    block_number: BlockNumber,
    block_timestamp: u64,
) -> Result<TradeRequest> {
    let token_amount_in = match open_trade_metadata.parsed_trade() {
        ParsedTrade::UniswapV2PairTrade(t) => {
            if pair.token_address < *config::WETH_ADDRESS {
                // token0 is token, token1 is weth
                t.amount0_out
            } else {
                // token1 is token, token0 is weth
                t.amount1_out
            }
        }
    };

    Ok(TradeRequest::UniswapV2(UniswapV2TradeRequest::new(
        pair.pair_address,
        pair.token_address,
        block_number,
        block_timestamp,
        pair.weth_reserve,
        pair.token_reserve,
        TradeRequestIntent::Close { token_amount_in },
    )))
}
