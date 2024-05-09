use super::TimePriceBarStore;

use crate::{config, primitives::Block, strategies::StrategyExecutor};

use alloy::{
    primitives::{Address, BlockNumber, U256},
    transports::Transport,
    network::Ethereum,
    providers::Provider,
};
use eyre::Result;
use std::sync::Arc;
use tokio::sync::oneshot;

pub struct IndexedUniswapV2Pair {
    pub token_reserve: U256,
    pub weth_reserve: U256,
    pub token_address: Address,
    pub pair_address: Address,
}

impl IndexedUniswapV2Pair {
    pub fn new(
        token_reserve: U256,
        weth_reserve: U256,
        token_address: Address,
        pair_address: Address,
    ) -> Self {
        Self {
            token_reserve,
            weth_reserve,
            token_address,
            pair_address,
        }
    }
}

pub struct IndexedBlockMessage {
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub uniswap_v2_pairs: Vec<IndexedUniswapV2Pair>,
    pub ack: Option<oneshot::Sender<()>>,
}

impl IndexedBlockMessage {
    pub fn new(
        block_number: BlockNumber,
        block_timestamp: u64,
        uniswap_v2_pairs: Vec<IndexedUniswapV2Pair>,
        ack: Option<oneshot::Sender<()>>,
    ) -> Self {
        Self {
            block_number,
            block_timestamp,
            uniswap_v2_pairs,
            ack,
        }
    }

    pub fn from_block(block: &Block) -> Self {
        Self::new(
            block.block_number,
            block.block_timestamp,
            block
                .uniswap_v2_pairs
                .iter()
                .filter_map(|(pair_address, pair)| match pair.trades.last() {
                    Some(trade) => {
                        let (token_reserve, weth_reserve) =
                            if pair.token_address < *config::WETH_ADDRESS {
                                (trade.reserve0, trade.reserve1)
                            } else {
                                (trade.reserve1, trade.reserve0)
                            };

                        Some(IndexedUniswapV2Pair::new(
                            token_reserve,
                            weth_reserve,
                            pair.token_address,
                            *pair_address,
                        ))
                    }
                    None => None,
                })
                .collect(),
            None,
        )
    }
}

pub trait Indexer<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    async fn exec(&mut self, strategy_executor: StrategyExecutor<T, P>) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        abi::IUniswapV2Pair, indexer::BlockBuilder,
        providers::rpc_provider::new_http_signer_provider,
    };

    use alloy::{
        primitives::{address, uint},
        rpc::types::eth::Filter,
        sol_types::SolEvent,
    };
    use eyre::Result;

    #[tokio::test]
    pub async fn test_from_block() -> Result<()> {
        let rpc_provider = Arc::new(new_http_signer_provider(&config::RPC_URL, None).await?);
        let block_number = 12822402;
        let block_timestamp = 100000;

        let logs_filter = Filter::new()
            .from_block(block_number)
            .to_block(block_number)
            .event_signature(vec![
                IUniswapV2Pair::Sync::SIGNATURE_HASH,
                IUniswapV2Pair::Swap::SIGNATURE_HASH,
            ]);

        let logs = rpc_provider.get_logs(&logs_filter).await?;

        let parsed_block = BlockBuilder::new(block_number, block_timestamp, &logs)
            .build(&rpc_provider)
            .await?;

        let result = IndexedBlockMessage::from_block(&parsed_block);

        assert_eq!(result.block_number, block_number);
        assert_eq!(result.uniswap_v2_pairs.len(), 4);

        let pair = result
            .uniswap_v2_pairs
            .into_iter()
            .find(|pair| pair.pair_address == address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
            .expect("Expected trades for pair");

        assert_eq!(pair.weth_reserve, uint!(24241863659908185248_U256));
        assert_eq!(pair.token_reserve, uint!(43340478928260732_U256));

        Ok(())
    }
}
