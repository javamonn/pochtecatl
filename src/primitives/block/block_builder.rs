use crate::{
    primitives::{Block, IndexedTrade, PairInput},
    providers::RpcProvider,
};

use alloy::{
    network::Ethereum,
    primitives::{BlockHash, BlockNumber},
    providers::Provider,
    rpc::types::eth::Log,
    transports::Transport,
};

use eyre::Result;
use tracing::{instrument, warn};

pub struct BlockBuilder {
    pub block_number: BlockNumber,
    pub block_hash: Option<BlockHash>,
    pub block_timestamp: u64,
    pub indexed_trades: Vec<IndexedTrade>,
}

impl BlockBuilder {
    pub fn new(block_number: BlockNumber, default_block_timestamp: u64, logs: &Vec<Log>) -> Self {
        let block_hash = logs.iter().find_map(|l| l.block_hash);
        let block_timestamp = logs
            .iter()
            .find_map(|l| l.block_timestamp)
            .unwrap_or(default_block_timestamp);
        let indexed_trades = IndexedTrade::from_logs(&logs);

        Self {
            block_number,
            block_hash,
            block_timestamp,
            indexed_trades,
        }
    }

    #[instrument(skip_all)]
    pub async fn build_many<T, P>(
        block_builders: Vec<Self>,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<Vec<Block>>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let pair_inputs = block_builders
            .iter()
            .flat_map(|builder| {
                builder
                    .indexed_trades
                    .iter()
                    .map(|trade| PairInput::from(trade))
            })
            .collect::<Vec<_>>();

        let pairs = rpc_provider
            .indexed_trade_provider()
            .get_pairs(
                pair_inputs,
                block_builders.last().map(|b| b.block_number.into()),
            )
            .await?;

        let blocks = block_builders
            .into_iter()
            .map(|builder| {
                builder.indexed_trades.into_iter().fold(
                    Block::new(
                        builder.block_hash,
                        builder.block_number,
                        builder.block_timestamp,
                    ),
                    |mut block, trade| {
                        match pairs.get(trade.pair_address()) {
                            Some(pair) => {
                                block.add_trade(trade, pair.token_address());
                            }
                            None => {
                                warn!(
                                    pair_address = trade.pair_address().to_string(),
                                    "invalid pair",
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
}

#[cfg(test)]
mod tests {
    use super::BlockBuilder;
    use crate::{
        config,
        primitives::{
            DexIndexedTrade, IndexedTrade, PairBlockTick, TickData, UniswapV2IndexedTrade,
            UniswapV2PairBlockTick,
        },
        providers::rpc_provider::new_http_signer_provider,
    };

    use alloy::{
        primitives::{address, uint},
        rpc::types::eth::Filter,
    };

    use eyre::Result;
    use fraction::BigUint;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_build_many() -> Result<()> {
        let rpc_provider = Arc::new(new_http_signer_provider(&config::RPC_URL, None).await?);
        let block_number = 12822402;
        let mock_timestamp = 100000;

        let logs_filter = Filter::new()
            .from_block(block_number)
            .to_block(block_number)
            .event_signature(IndexedTrade::event_signature_hashes());

        let logs = rpc_provider.get_logs(&logs_filter).await?;

        let builder = BlockBuilder::new(block_number.into(), mock_timestamp, &logs);

        let block = &BlockBuilder::build_many(vec![builder], &rpc_provider).await?[0];

        assert_eq!(block.block_number, block_number);
        assert_eq!(block.block_timestamp, mock_timestamp);
        assert_eq!(block.pair_ticks.len(), 9);

        let pair = block
            .pair_ticks
            .get(&address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
            .expect("Expected trades for pair");

        let expected_trades = vec![
            UniswapV2IndexedTrade::new(
                address!("c1c52be5c93429be50f5518a582f690d0fc0528a"),
                uint!(0_U256),
                uint!(196648594373849_U256),
                uint!(110094173315701195_U256),
                uint!(0_U256),
                uint!(24234363659908185248_U256),
                uint!(43353851609950831_U256),
                address!("1Fba6b0BBae2B74586fBA407Fb45Bd4788B7b130"),
            ),
            UniswapV2IndexedTrade::new(
                address!("c1c52be5c93429be50f5518a582f690d0fc0528a"),
                uint!(7500000000000000_U256),
                uint!(0_U256),
                uint!(0_U256),
                uint!(13372681690099_U256),
                uint!(24241863659908185248_U256),
                uint!(43340478928260732_U256),
                address!("7381C38985dA304eBA18fCef5E1f6e9fA0798b84"),
            ),
        ];

        let token_address = address!("F7669AC505D8Eb518103fEDa96A7A12737794492");
        assert_eq!(
            *pair,
            PairBlockTick::UniswapV2(UniswapV2PairBlockTick {
                token_address,
                makers: vec![
                    address!("1Fba6b0BBae2B74586fBA407Fb45Bd4788B7b130"),
                    address!("7381C38985dA304eBA18fCef5E1f6e9fA0798b84")
                ],
                reserve0: uint!(24241863659908185248_U256),
                reserve1: uint!(43340478928260732_U256),
                tick: TickData::new(
                    expected_trades[0].token_price_before(&token_address),
                    expected_trades[0].token_price_before(&token_address),
                    expected_trades[0].token_price_after(&token_address),
                    expected_trades[1].token_price_after(&token_address),
                    BigUint::from(117594173315701195_u128)
                )
            })
        );

        Ok(())
    }
}
