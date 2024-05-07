use super::{Block, ParseableTrade, UniswapV2PairTrade};

use crate::providers::RpcProvider;

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockHash, BlockNumber},
    providers::Provider,
    rpc::types::eth::Log,
    transports::Transport,
};

use eyre::Result;
use fnv::FnvHashSet;
use std::sync::Arc;
use tracing::{warn, instrument};

pub struct BlockBuilder {
    pub block_number: BlockNumber,
    pub block_hash: Option<BlockHash>,
    pub block_timestamp: u64,
    pub uniswap_v2_pairs: Vec<(Address, UniswapV2PairTrade)>,
}

impl BlockBuilder {
    pub fn new(block_number: BlockNumber, default_block_timestamp: u64, logs: &Vec<Log>) -> Self {
        let mut block_hash = None;
        let mut block_timestamp = None;
        let mut uniswap_v2_pairs = Vec::new();

        for (idx, log) in logs.iter().enumerate() {
            // Try to set block hash if log contains it
            if block_hash.is_none() && log.block_hash.is_some() {
                block_hash = log.block_hash;
            }

            // Try to set block timestamp if log contains it
            if block_timestamp.is_none() && log.block_timestamp.is_some() {
                block_timestamp = log.block_timestamp;
            }

            // Try to parse a uniswap v2 trade
            if let Some(uniswap_v2_pair_trade) = UniswapV2PairTrade::parse_from_log(log, logs, idx)
            {
                uniswap_v2_pairs.push((log.address(), uniswap_v2_pair_trade));
            }
        }

        Self {
            block_number,
            block_hash,
            block_timestamp: block_timestamp.unwrap_or(default_block_timestamp),
            uniswap_v2_pairs,
        }
    }

    #[instrument(skip_all)]
    pub async fn build_many<T, P>(
        block_builders: Vec<Self>,
        rpc_provider: Arc<RpcProvider<T, P>>,
    ) -> Result<Vec<Block>>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let uniswap_v2_pair_token_addresses = {
            let mut pair_addresses = FnvHashSet::default();
            block_builders.iter().for_each(|builder| {
                builder
                    .uniswap_v2_pairs
                    .iter()
                    .for_each(|(pair_address, _)| {
                        pair_addresses.insert(*pair_address);
                    })
            });

            rpc_provider
                .uniswap_v2_pair_provider()
                .get_token_addresses(
                    pair_addresses.into_iter().collect(),
                    block_builders.last().map(|b| b.block_number.into()),
                )
                .await
        }?;

        let blocks = block_builders
            .into_iter()
            .map(|builder| {
                builder.uniswap_v2_pairs.into_iter().fold(
                    Block::new(
                        builder.block_hash,
                        builder.block_number,
                        builder.block_timestamp,
                    ),
                    |mut block, (pair_address, trade)| {
                        match uniswap_v2_pair_token_addresses.get(&pair_address) {
                            Some((token0_address, token1_address)) => {
                                block.add_uniswap_v2_pair_trade(
                                    pair_address,
                                    trade,
                                    token0_address,
                                    token1_address,
                                );
                            }
                            None => {
                                warn!(
                                    pair_address = pair_address.to_string(),
                                    "failed to get token addresses",
                                );
                            }
                        }

                        block
                    },
                )
            })
            .collect();

        Ok(blocks)
    }

    pub async fn build<T, P>(self, rpc_provider: Arc<RpcProvider<T, P>>) -> Result<Block>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let uniswap_v2_pair_token_addresses = rpc_provider
            .uniswap_v2_pair_provider()
            .get_token_addresses(
                self.uniswap_v2_pairs
                    .iter()
                    .map(|(pair_address, _)| *pair_address)
                    .collect(),
                Some(self.block_number.into()),
            )
            .await?;

        let block = self.uniswap_v2_pairs.into_iter().fold(
            Block::new(self.block_hash, self.block_number, self.block_timestamp),
            |mut block, (pair_address, trade)| {
                match uniswap_v2_pair_token_addresses.get(&pair_address) {
                    Some((token0_address, token1_address)) => {
                        block.add_uniswap_v2_pair_trade(
                            pair_address,
                            trade,
                            token0_address,
                            token1_address,
                        );
                    }
                    None => {
                        warn!(
                            pair_address = pair_address.to_string(),
                            "failed to get token addresses",
                        );
                    }
                }

                block
            },
        );

        Ok(block)
    }
}
