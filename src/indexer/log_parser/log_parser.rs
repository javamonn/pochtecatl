use super::{uniswap_v2_pair_swap, uniswap_v2_pair_sync, Block, UniswapV2PairTrade};

use alloy::{
    primitives::U256,
    rpc::types::eth::{Header, Log},
};
use ruint::uint;
use eyre::Result;

fn parse_uniswap_v2_trade(
    block: &mut Block,
    logs: &Vec<&Log>,
    log: &Log,
    relative_log_idx: usize,
) -> bool {
    if let Some(parsed_swap) = uniswap_v2_pair_swap::parse(&log) {
        let prev_parsed_sync_log = logs.get(relative_log_idx - 1).and_then(|prev_log| {
            // Ensure prev log in the arr is the previous log index in the same block for the same
            // pair
            if prev_log.block_hash == log.block_hash
                && prev_log.address == log.address
                && prev_log.log_index.is_some_and(|prev_log_index| {
                    log.log_index
                        .is_some_and(|log_index| prev_log_index + uint!(1_U256) == log_index)
                })
            {
                uniswap_v2_pair_sync::parse(&prev_log)
            } else {
                None
            }
        });

        if let Some(parsed_sync) = prev_parsed_sync_log {
            block
                .uniswap_v2_trades
                .entry(parsed_swap.address)
                .and_modify(|trades| {
                    trades.push(UniswapV2PairTrade::new(
                        parsed_swap.amount0In,
                        parsed_swap.amount1In,
                        parsed_swap.amount0Out,
                        parsed_swap.amount1Out,
                        U256::from(parsed_sync.reserve0),
                        U256::from(parsed_sync.reserve1),
                        parsed_swap.sender,
                    ));
                })
                .or_insert_with(|| {
                    let pair_trades = vec![UniswapV2PairTrade::new(
                        parsed_swap.amount0In,
                        parsed_swap.amount1In,
                        parsed_swap.amount0Out,
                        parsed_swap.amount1Out,
                        U256::from(parsed_sync.reserve0),
                        U256::from(parsed_sync.reserve1),
                        parsed_swap.sender,
                    )];
                    pair_trades
                });
            true
        } else {
            false
        }
    } else {
        false
    }
}

pub fn parse(block_header: &Header, logs: Vec<&Log>) -> Result<Block> {
    let mut block: Block = block_header.try_into()?;

    for (idx, log) in logs.iter().enumerate() {
        // Try to parse a uniswap v2 trade
        if parse_uniswap_v2_trade(&mut block, &logs, log, idx) {
            continue;
        }
    }

    Ok(block)
}
