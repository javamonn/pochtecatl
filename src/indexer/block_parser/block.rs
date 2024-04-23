use super::{ParseableTrade, UniswapV2PairTrade};

use crate::{config, rpc_provider::RpcProvider};

use alloy::{
    primitives::{Address, BlockHash, BlockNumber},
    rpc::types::eth::{Header, Log},
};

use eyre::{eyre, Result};
use fnv::FnvHashMap;
use std::sync::Arc;
use tokio::task::JoinSet;

pub struct UniswapV2Pair {
    pub token_address: Address,
    pub trades: Vec<UniswapV2PairTrade>,
}

impl UniswapV2Pair {
    pub fn new(token_address: Address, trades: Vec<UniswapV2PairTrade>) -> Self {
        Self {
            token_address,
            trades,
        }
    }
}

pub struct Block {
    pub block_hash: BlockHash,
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub uniswap_v2_pairs: FnvHashMap<Address, UniswapV2Pair>,
}

impl Block {
    pub fn new(block_hash: BlockHash, block_number: BlockNumber, block_timestamp: u64) -> Self {
        Self {
            block_hash,
            block_number,
            block_timestamp,
            uniswap_v2_pairs: FnvHashMap::default(),
        }
    }

    pub async fn parse(
        rpc_provider: Arc<RpcProvider>,
        header: &Header,
        logs: &Vec<Log>,
    ) -> Result<Self> {
        let mut uniswap_v2_pairs = Vec::new();
        for (idx, log) in logs.iter().enumerate() {
            // Try to parse a uniswap v2 trade
            if let Some(uniswap_v2_pair_trade) = UniswapV2PairTrade::parse_from_log(log, logs, idx)
            {
                uniswap_v2_pairs.push((log.address(), uniswap_v2_pair_trade));
            }
        }

        let uniswap_v2_pair_token_addresses = get_block_uniswap_v2_pair_token_addresses(
            rpc_provider,
            uniswap_v2_pairs
                .iter()
                .map(|(pair_address, _)| *pair_address),
        )
        .await;

        let block: Block = uniswap_v2_pairs.into_iter().fold(
            header.try_into()?,
            |mut block, (pair_address, trade)| {
                match uniswap_v2_pair_token_addresses.get(&pair_address) {
                    Some((token0_address, token1_address)) => {
                        let token_address = if *token0_address == *config::WETH_ADDRESS {
                            Some(*token1_address)
                        } else if *token1_address == *config::WETH_ADDRESS {
                            Some(*token0_address)
                        } else {
                            None
                        };

                        if let Some(token_address) = token_address {
                            let uniswap_v2_pair = block
                                .uniswap_v2_pairs
                                .entry(pair_address)
                                .or_insert_with(|| UniswapV2Pair::new(token_address, Vec::new()));
                            uniswap_v2_pair.trades.push(trade);
                        }
                    }
                    None => {
                        log::warn!(
                            block_number = block.block_number.to_string(),
                            block_hash = block.block_hash.to_string();
                            "failed to get pair token addresses for pair_address: {}",
                            pair_address
                        );
                    }
                }

                block
            },
        );

        Ok(block)
    }
}

impl TryFrom<&Header> for Block {
    type Error = eyre::Report;

    fn try_from(header: &Header) -> Result<Self, Self::Error> {
        match (header.hash, header.number) {
            (None, _) => Err(eyre!("header is missing hash")),
            (_, None) => Err(eyre!("header is missing number")),
            (Some(hash), Some(number)) => Ok(Block::new(
                hash,
                number.to::<u64>(),
                header.timestamp.to::<u64>(),
            )),
        }
    }
}

async fn get_block_uniswap_v2_pair_token_addresses<I>(
    rpc_provider: Arc<RpcProvider>,
    pair_addresses: I,
) -> FnvHashMap<Address, (Address, Address)>
where
    I: Iterator<Item = Address>,
{
    let mut tasks = JoinSet::new();
    let mut output = FnvHashMap::default();

    for pair_address in pair_addresses {
        let rpc_provider = Arc::clone(&rpc_provider);
        tasks.spawn(async move {
            match rpc_provider
                .get_uniswap_v2_pair_token_addresses(pair_address)
                .await
            {
                Ok(token_addresses) => Some((pair_address, token_addresses)),
                Err(err) => {
                    log::error!(
                        "get_uniswap_v2_pair_token_addresses {} failed: {:?}",
                        pair_address,
                        err
                    );
                    None
                }
            }
        });
    }

    while let Some(pair_token_addresses) = tasks.join_next().await {
        match pair_token_addresses {
            Ok(Some((pair_address, token_addresses))) => {
                output.insert(pair_address, token_addresses);
            }
            Ok(None) => {}
            Err(err) => {
                log::error!("join_next error: {:?}", err);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::Block;

    use crate::{
        abi::IUniswapV2Pair, config, indexer::block_parser::UniswapV2PairTrade,
        rpc_provider::RpcProvider,
    };

    use alloy::{
        primitives::{address, uint},
        rpc::types::eth::Filter,
        sol_types::SolEvent,
    };
    use eyre::OptionExt;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_parse() -> eyre::Result<()> {
        let rpc_provider = Arc::new(RpcProvider::new(&config::RPC_URL).await?);
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

        let parsed_block = Block::parse(rpc_provider, &header, &logs).await?;

        assert_eq!(parsed_block.block_number, block_number);
        assert_eq!(parsed_block.block_timestamp, 1712434151);
        assert_eq!(parsed_block.uniswap_v2_pairs.len(), 4);

        let pair = parsed_block
            .uniswap_v2_pairs
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
                address!("7381C38985dA304eBA18fCef5E1f6e9fA0798b84"),
            ),
        ];

        assert_eq!(pair.trades, expected_trades);

        Ok(())
    }
}
