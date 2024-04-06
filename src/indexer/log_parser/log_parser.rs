use super::{uniswap_v2_pair_swap, uniswap_v2_pair_sync, Block, UniswapV2PairTrade};

use alloy::{
    primitives::U256,
    rpc::types::eth::{Header, Log},
};
use eyre::Result;
use ruint::uint;

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
                        parsed_swap.to,
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
                        parsed_swap.to,
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

#[cfg(test)]
mod tests {
    use super::parse;

    use crate::{
        abi::IUniswapV2Pair, config, indexer::log_parser::UniswapV2PairTrade,
        rpc_provider::RpcProvider,
    };

    use alloy::{
        primitives::{address, uint},
        rpc::types::eth::Filter,
        sol_types::SolEvent,
    };
    use eyre::OptionExt;

    #[tokio::test]
    async fn test_parse() -> eyre::Result<()> {
        let rpc_provider = RpcProvider::new(&config::RPC_URL).await?;

        let block_number = 12822402;
        let logs_filter = Filter::new()
            .from_block(block_number)
            .to_block(block_number)
            .event_signature(vec![
                IUniswapV2Pair::Sync::SIGNATURE_HASH,
                IUniswapV2Pair::Swap::SIGNATURE_HASH,
            ]);

        let (header, logs) = {
            let (header_result, logs_result) = tokio::join!(
                rpc_provider.get_block_header(block_number),
                rpc_provider.get_logs(&logs_filter)
            );

            (
                header_result.and_then(|header| header.ok_or_eyre("Missing block"))?,
                logs_result?,
            )
        };

        let parsed_block = parse(&header, logs.iter().collect())?;

        assert_eq!(parsed_block.block_number, block_number);
        assert_eq!(parsed_block.block_timestamp, 1712434151);
        assert_eq!(parsed_block.uniswap_v2_trades.len(), 4);

        let trades = parsed_block
            .uniswap_v2_trades
            .get(&address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
            .expect("Expected trades for pair");

        let expected_trades = vec![
            UniswapV2PairTrade::new(
                uint!(0_U256),
                uint!(196648594373849_U256),
                uint!(110094173315701195_U256),
                uint!(0_U256),
                uint!(24234363659908185248_U256),
                uint!(43353851609950831_U256),
                address!("1Fba6b0BBae2B74586fBA407Fb45Bd4788B7b130"),
            ),
            UniswapV2PairTrade::new(
                uint!(7500000000000000_U256),
                uint!(0_U256),
                uint!(0_U256),
                uint!(13372681690099_U256),
                uint!(24241863659908185248_U256),
                uint!(43340478928260732_U256),
                address!("7381C38985dA304eBA18fCef5E1f6e9fA0798b84")
            )
        ];

        assert_eq!(trades, &expected_trades);

        Ok(())
    }
}
