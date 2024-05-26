use pochtecatl_primitives::{
    constants, Block, IndicatorsConfig, Resolution, ResolutionTimestamp, RpcProvider, TimePriceBars,
};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockNumber},
    providers::Provider,
    transports::Transport,
};

use eyre::{Context, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, instrument, warn};

pub struct TimePriceBarStore {
    resolution: Resolution,
    time_price_bars: RwLock<FnvHashMap<Address, TimePriceBars>>,
    retention_count: u64,
    is_backtest: bool,

    last_inserted_block_number: RwLock<Option<BlockNumber>>,
    last_pruned_at_block_number: RwLock<Option<BlockNumber>>,
}

impl TimePriceBarStore {
    pub fn new(resolution: Resolution, retention_count: u64, is_backtest: bool) -> Self {
        Self {
            resolution,
            time_price_bars: RwLock::new(FnvHashMap::default()),
            retention_count,
            is_backtest,
            last_inserted_block_number: RwLock::new(None),
            last_pruned_at_block_number: RwLock::new(None),
        }
    }

    pub fn time_price_bars(&self) -> &RwLock<FnvHashMap<Address, TimePriceBars>> {
        &self.time_price_bars
    }

    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }

    #[cfg(test)]
    pub fn last_inserted_block_number(&self) -> Option<BlockNumber> {
        *self.last_inserted_block_number.read().unwrap()
    }

    #[instrument(skip_all)]
    pub async fn insert_block<T, P>(
        &self,
        rpc_provider: Arc<RpcProvider<T, P>>,
        block: &Block,
    ) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let block_resolution_timestamp =
            ResolutionTimestamp::from_timestamp(block.block_timestamp, &self.resolution);

        // The resolution timestamp of the time price bar containing the most recent
        // finalized block. A TimePriceBars collection can safely finalize any time
        // price bars up to this timestamp.
        let finalized_timestamp = if self.is_backtest {
            Some(block_resolution_timestamp.previous(&self.resolution))
        } else {
            rpc_provider
                .block_provider()
                .get_finalized_block_header()
                .await
                .inspect_err(|err| warn!("Failed to get finalized block header: {:?}", err))
                .ok()
                .map(|finalized_block_header| {
                    ResolutionTimestamp::from_timestamp(
                        finalized_block_header.timestamp.to::<u64>(),
                        &self.resolution,
                    )
                    .previous(&self.resolution)
                })
        };

        // Insert the new block price bars
        {
            let mut time_price_bars = self.time_price_bars.write().unwrap();

            // If this block is behind the last inserted block number, this is a reorg
            // and we need to prune existing data
            {
                let last_inserted_block_number = self.last_inserted_block_number.read().unwrap();
                match *last_inserted_block_number {
                    Some(last_inserted_block_number)
                        if block.block_number <= last_inserted_block_number =>
                    {
                        let mut pair_addresses_to_remove = Vec::new();
                        for (pair_address, pair_time_price_bars) in time_price_bars.iter_mut() {
                            pair_time_price_bars
                                .prune_to_reorged_block_number(block.block_number)?;

                            if pair_time_price_bars.is_empty() {
                                pair_addresses_to_remove.push(pair_address.clone());
                            }
                        }

                        for pair_address in pair_addresses_to_remove.iter() {
                            time_price_bars.remove(pair_address);
                        }

                        debug!(
                            block_number = block.block_number,
                            "pruned time price bars due to reorg"
                        );
                    }
                    _ => {}
                }
            }

            // Insert new BlockPriceBar items into time_price_bars
            for (pair_address, pair) in block.pair_ticks.iter() {
                let time_price_bars =
                    time_price_bars
                        .entry(pair_address.clone())
                        .or_insert_with(|| {
                            TimePriceBars::new(
                                Some(self.retention_count),
                                self.resolution,
                                Some(IndicatorsConfig::All),
                            )
                        });

                time_price_bars
                    .insert_data(
                        block.block_number,
                        pair.tick().clone(),
                        block.block_timestamp,
                        finalized_timestamp,
                    )
                    .wrap_err_with(|| {
                        format!(
                            "Failed to insert new block price bar for pair {}",
                            pair_address
                        )
                    })?
            }

            // Prune any stale time price bars
            {
                let mut last_pruned_at_block_number =
                    self.last_pruned_at_block_number.write().unwrap();
                match last_pruned_at_block_number.as_ref() {
                    Some(last_pruned_at_block_number_value)
                        if block.block_number
                            > last_pruned_at_block_number_value
                                + (self.resolution.offset()
                                    / constants::AVERAGE_BLOCK_TIME_SECONDS) =>
                    {
                        // Prune time price bars
                        let stale_pair_addresses = time_price_bars
                            .iter()
                            .filter_map(|(pair_address, pair_time_price_bars)| {
                                if pair_time_price_bars.is_stale(block_resolution_timestamp) {
                                    Some(pair_address.clone())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<Address>>();

                        for pair_address in stale_pair_addresses.into_iter() {
                            debug!(
                                pair_address = pair_address.to_string(),
                                "pruning time price bar"
                            );
                            time_price_bars.remove(&pair_address);
                        }

                        last_pruned_at_block_number.replace(block.block_number);
                    }
                    None => {
                        // Set initial last pruned at to the current block number
                        last_pruned_at_block_number.replace(block.block_number);
                    }
                    Some(_) => { /* noop: we do not need to prune yet */ }
                }
            }
        }

        // Update the last inserted block number
        {
            let mut last_inserted_block_number = self.last_inserted_block_number.write().unwrap();
            *last_inserted_block_number = Some(block.block_number)
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TimePriceBarStore;
    use crate::{config, indexer::BlockChunk};

    use pochtecatl_primitives::{
        new_http_signer_provider, Block, BlockBuilder, IndexedTrade, Resolution,
        ResolutionTimestamp, RpcProvider, TTLCache,
    };

    use alloy::{
        network::Ethereum,
        primitives::{address, uint, BlockNumber, B256},
        providers::Provider,
        rpc::types::eth::Filter,
        transports::Transport,
    };

    use eyre::{OptionExt, Result};
    use hex_literal::hex;
    use std::sync::Arc;

    async fn get_block<T, P>(
        rpc_provider: Arc<RpcProvider<T, P>>,
        block_number: BlockNumber,
    ) -> Result<Block>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let logs_filter = Filter::new()
            .from_block(block_number)
            .to_block(block_number)
            .event_signature(IndexedTrade::event_signature_hashes());

        let (header, logs) = {
            let (header_result, logs_result) = tokio::join!(
                rpc_provider.block_provider().get_block_header(block_number),
                rpc_provider.get_logs(&logs_filter)
            );

            (
                header_result.and_then(|header| header.ok_or_eyre("Missing block"))?,
                logs_result?,
            )
        };

        BlockBuilder::build_many(
            vec![BlockBuilder::new(
                block_number,
                header.timestamp.to::<u64>(),
                &logs,
            )],
            &rpc_provider,
        )
        .await
        .map(|mut blocks| blocks.swap_remove(0))
    }

    #[tokio::test]
    async fn test_insert_blocks() -> Result<()> {
        let store = TimePriceBarStore::new(Resolution::FiveMinutes, 60, true);
        let rpc_provider = Arc::new(
            new_http_signer_provider(
                url::Url::parse(config::RPC_URL.as_str())?,
                &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
                None,
                true,
            )
            .await?,
        );
        let block_chunk = BlockChunk::fetch_from_rpc(&rpc_provider, 14943872, 14944775).await?;

        for block in block_chunk.data.iter() {
            store
                .insert_block(Arc::clone(&rpc_provider), &block)
                .await?;
        }

        {
            let time_price_bars = store.time_price_bars.read().unwrap();
            let pair_time_price_bars = time_price_bars
                .get(&address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA"))
                .expect("Expected pair time price bars, but found None");

            let last_timestamp = ResolutionTimestamp::from_timestamp(
                block_chunk
                    .data
                    .last()
                    .expect("Expected block")
                    .block_timestamp,
                &Resolution::FiveMinutes,
            );
            let last_time_price_bar = pair_time_price_bars
                .data()
                .get(&last_timestamp)
                .expect("Expected last time price bar for pair");

            assert_eq!(
                last_time_price_bar.close().to_string(),
                "0.0000062062363659129413654159"
            );
            assert_eq!(
                last_time_price_bar
                    .indicators()
                    .expect("Expected indicators")
                    .ema
                    .0
                    .to_string(),
                "0.00000621978880518296242568777",
            )
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_block_backtest() -> Result<()> {
        let mock_finalized_timestamp = uint!(1000_U256);
        let rpc_provider = {
            let inner = Arc::new(
                new_http_signer_provider(
                    url::Url::parse(config::RPC_URL.as_str())?,
                    &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318")
                        .into(),
                    None,
                    true,
                )
                .await?,
            );
            let mock_finalized_block_number = 12822402;
            let mut mock_finalized_header = inner
                .block_provider()
                .get_block_header(mock_finalized_block_number)
                .await?
                .expect("Expected block header, but found None");

            mock_finalized_header.timestamp = mock_finalized_timestamp.clone();

            Arc::new(
                new_http_signer_provider(
                    url::Url::parse(config::RPC_URL.as_str())?,
                    &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318")
                        .into(),
                    Some(TTLCache::new(mock_finalized_header, None)),
                    true,
                )
                .await?,
            )
        };

        // test base case
        {
            let store = TimePriceBarStore::new(Resolution::FiveMinutes, 5, true);
            let block = get_block(Arc::clone(&rpc_provider), 12822402).await?;
            store
                .insert_block(Arc::clone(&rpc_provider), &block)
                .await?;

            assert_eq!(store.last_inserted_block_number(), Some(block.block_number));
            assert_eq!(
                store
                    .time_price_bars()
                    .read()
                    .unwrap()
                    .get(&address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
                    .unwrap()
                    .last_finalized_timestamp()
                    .clone(),
                Some(
                    ResolutionTimestamp::from_timestamp(
                        block.block_timestamp,
                        &Resolution::FiveMinutes
                    )
                    .previous(&Resolution::FiveMinutes)
                )
            );
            assert_eq!(
                store
                    .time_price_bars()
                    .read()
                    .unwrap()
                    .get(&address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
                    .unwrap()
                    .time_price_bar_range(
                        &ResolutionTimestamp::zero(),
                        &ResolutionTimestamp::from_timestamp(
                            block.block_timestamp,
                            &Resolution::FiveMinutes
                        )
                    )
                    .len(),
                1
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_block_peak() -> Result<()> {
        let mock_finalized_timestamp = uint!(1000_U256);
        let rpc_provider = {
            let inner = Arc::new(
                new_http_signer_provider(
                    url::Url::parse(config::RPC_URL.as_str())?,
                    &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318")
                        .into(),
                    None,
                    false,
                )
                .await?,
            );
            let mock_finalized_block_number = 12822402;
            let mut mock_finalized_header = inner
                .block_provider()
                .get_block_header(mock_finalized_block_number)
                .await?
                .expect("Expected block header, but found None");

            mock_finalized_header.timestamp = mock_finalized_timestamp.clone();

            Arc::new(
                new_http_signer_provider(
                    url::Url::parse(config::RPC_URL.as_str())?,
                    &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318")
                        .into(),
                    Some(TTLCache::new(mock_finalized_header, None)),
                    false,
                )
                .await?,
            )
        };

        // test base case
        {
            let store = TimePriceBarStore::new(Resolution::FiveMinutes, 5, false);
            let block = get_block(Arc::clone(&rpc_provider), 12822402).await?;
            store
                .insert_block(Arc::clone(&rpc_provider), &block)
                .await?;

            assert_eq!(store.last_inserted_block_number(), Some(block.block_number));
            assert_eq!(
                store
                    .time_price_bars()
                    .read()
                    .unwrap()
                    .get(&address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
                    .unwrap()
                    .last_finalized_timestamp()
                    .clone(),
                Some(
                    ResolutionTimestamp::from_timestamp(
                        mock_finalized_timestamp.to::<u64>(),
                        &Resolution::FiveMinutes
                    )
                    .previous(&Resolution::FiveMinutes)
                )
            );
            assert_eq!(
                store
                    .time_price_bars()
                    .read()
                    .unwrap()
                    .get(&address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
                    .unwrap()
                    .time_price_bar_range(
                        &ResolutionTimestamp::zero(),
                        &ResolutionTimestamp::from_timestamp(
                            block.block_timestamp,
                            &Resolution::FiveMinutes
                        )
                    )
                    .len(),
                1
            );
        }

        // test reorg handling
        {
            let store = TimePriceBarStore::new(Resolution::FiveMinutes, 5, false);
            let mut blocks = Vec::new();

            // Insert blocks in order
            for block_number in 12822402..=12822404 {
                let block = get_block(Arc::clone(&rpc_provider), block_number).await?;
                store
                    .insert_block(Arc::clone(&rpc_provider), &block)
                    .await?;
                blocks.push(block);
            }

            assert_eq!(store.last_inserted_block_number(), Some(12822404));

            // Re-insert the 03 block to trigger the reorg logic
            store
                .insert_block(Arc::clone(&rpc_provider), &blocks[1])
                .await?;

            assert_eq!(store.last_inserted_block_number(), Some(12822403));
        }

        Ok(())
    }
}
