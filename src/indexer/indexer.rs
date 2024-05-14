use crate::strategies::StrategyExecutor;

#[cfg(test)]
use crate::config;

use alloy::{network::Ethereum, providers::Provider, transports::Transport};
use eyre::Result;

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
        primitives::{BlockBuilder, BlockMessage, DexPair, IndexedTrade, Pair},
        providers::rpc_provider::new_http_signer_provider,
    };

    use alloy::{primitives::address, rpc::types::eth::Filter};
    use eyre::Result;
    use std::sync::Arc;

    #[tokio::test]
    pub async fn test_from_block() -> Result<()> {
        let rpc_provider = Arc::new(new_http_signer_provider(&config::RPC_URL, None).await?);
        let block_number = 12822402;
        let block_timestamp = 100000;

        let logs_filter = Filter::new()
            .from_block(block_number)
            .to_block(block_number)
            .event_signature(IndexedTrade::event_signature_hashes());

        let logs = rpc_provider.get_logs(&logs_filter).await?;

        let parsed_block = BlockBuilder::build_many(
            vec![BlockBuilder::new(block_number, block_timestamp, &logs)],
            &rpc_provider,
        )
        .await?
        .swap_remove(0);

        let result = BlockMessage::from(parsed_block);

        assert_eq!(result.block_number, block_number);
        assert_eq!(result.pairs.len(), 9);

        let pair = result
            .pairs
            .into_iter()
            .find_map(|pair| match pair {
                Pair::UniswapV2(pair)
                    if *pair.address() == address!("c1c52be5c93429be50f5518a582f690d0fc0528a") =>
                {
                    Some(pair)
                }
                _ => None,
            })
            .expect("Expected trades for pair");

        assert_eq!(
            *pair.token_address(),
            address!("F7669AC505D8Eb518103fEDa96A7A12737794492")
        );

        Ok(())
    }
}
