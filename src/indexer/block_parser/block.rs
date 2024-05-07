use super::{BlockBuilder, UniswapV2PairTrade};

use crate::{config, providers::RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockHash, BlockNumber},
    providers::Provider,
    rpc::types::eth::Log,
    transports::Transport,
};

use eyre::Result;
use fnv::FnvHashMap;
use std::sync::Arc;

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
    pub block_hash: Option<BlockHash>,
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub uniswap_v2_pairs: FnvHashMap<Address, UniswapV2Pair>,
}

impl Block {
    pub fn new(
        block_hash: Option<BlockHash>,
        block_number: BlockNumber,
        block_timestamp: u64,
    ) -> Self {
        Self {
            block_hash,
            block_number,
            block_timestamp,
            uniswap_v2_pairs: FnvHashMap::default(),
        }
    }

    pub fn add_uniswap_v2_pair_trade(
        &mut self,
        pair_address: Address,
        trade: UniswapV2PairTrade,
        token0_address: &Address,
        token1_address: &Address,
    ) {
        let token_address = if *token0_address == *config::WETH_ADDRESS {
            Some(token1_address)
        } else if *token1_address == *config::WETH_ADDRESS {
            Some(token0_address)
        } else {
            None
        };

        if let Some(token_address) = token_address {
            let uniswap_v2_pair = self
                .uniswap_v2_pairs
                .entry(pair_address)
                .or_insert_with(|| UniswapV2Pair::new(*token_address, Vec::new()));
            uniswap_v2_pair.trades.push(trade);
        }
    }

    pub async fn parse<T, P>(
        rpc_provider: Arc<RpcProvider<T, P>>,
        block_number: BlockNumber,
        block_timestamp: u64,
        logs: &Vec<Log>,
    ) -> Result<Self>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        BlockBuilder::new(block_number, block_timestamp, logs)
            .build(rpc_provider)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::Block;

    use crate::{
        abi::IUniswapV2Pair, config, indexer::block_parser::UniswapV2PairTrade,
        providers::rpc_provider::new_http_signer_provider,
    };

    use alloy::{
        primitives::{address, uint},
        rpc::types::eth::Filter,
        sol_types::SolEvent,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn test_parse() -> eyre::Result<()> {
        let rpc_provider = Arc::new(new_http_signer_provider(&config::RPC_URL, None).await?);
        let block_number = 12822402;
        let mock_timestamp = 100000;

        let logs_filter = Filter::new()
            .from_block(block_number)
            .to_block(block_number)
            .event_signature(vec![
                IUniswapV2Pair::Sync::SIGNATURE_HASH,
                IUniswapV2Pair::Swap::SIGNATURE_HASH,
            ]);

        let logs = rpc_provider.get_logs(&logs_filter).await?;

        let parsed_block = Block::parse(rpc_provider, block_number, mock_timestamp, &logs).await?;

        assert_eq!(parsed_block.block_number, block_number);
        assert_eq!(parsed_block.block_timestamp, mock_timestamp);
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
