use super::{
    Block, BlockPriceBar, Resolution, ResolutionTimestamp, TimePriceBars,
};
use crate::{config, rpc_provider::RpcProvider};

use alloy::primitives::{Address, BlockNumber};

use eyre::{Context, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};

// In a backfill we can finalize up to the last completed time bar resolution tick,
// as we will never encounter a reorg. In peak, we can only finalize up to the
// actual finalized block.
async fn get_timestamp_range_to_finalize(
    rpc_provider: Arc<RpcProvider>,
    inserted_block_resolution_timestamp: &ResolutionTimestamp,
    last_finalized_timestamp: &Option<ResolutionTimestamp>,
    resolution: &Resolution,
) -> Option<(ResolutionTimestamp, ResolutionTimestamp)> {
    match rpc_provider.get_finalized_block_header().await {
        Ok(finalized_block_header) => {
            let end_finalized_timestamp = if *config::IS_BACKFILL {
                inserted_block_resolution_timestamp.previous(&resolution)
            } else {
                ResolutionTimestamp::from_timestamp(
                    finalized_block_header.timestamp.to::<u64>(),
                    &resolution,
                )
            };

            match last_finalized_timestamp {
                Some(last_finalized_timestamp) => {
                    if end_finalized_timestamp > *last_finalized_timestamp {
                        Some((
                            last_finalized_timestamp.next(&resolution),
                            end_finalized_timestamp,
                        ))
                    } else {
                        None
                    }
                }
                None => Some((ResolutionTimestamp::zero(), end_finalized_timestamp)),
            }
        }
        Err(err) => {
            log::warn!(
                "Failed to get finalized block header, skipping finalization: {:?}",
                err
            );
            None
        }
    }
}

pub struct TimePriceBarStore {
    resolution: Resolution,
    time_price_bars: RwLock<FnvHashMap<Address, TimePriceBars>>,
    last_finalized_timestamp: RwLock<Option<ResolutionTimestamp>>,
    last_inserted_block_number: RwLock<Option<BlockNumber>>,
    retention_count: u64,
}

impl TimePriceBarStore {
    pub fn new(resolution: Resolution, retention_count: u64) -> Self {
        Self {
            resolution,
            time_price_bars: RwLock::new(FnvHashMap::default()),
            last_finalized_timestamp: RwLock::new(None),
            last_inserted_block_number: RwLock::new(None),
            retention_count,
        }
    }

    #[cfg(test)]
    pub fn time_price_bars(&self) -> &RwLock<FnvHashMap<Address, TimePriceBars>> {
        &self.time_price_bars
    }

    #[cfg(test)]
    pub fn last_inserted_block_number(&self) -> Option<BlockNumber> {
        *self.last_inserted_block_number.read().unwrap()
    }

    #[cfg(test)]
    pub fn last_finalized_timestamp(&self) -> Option<ResolutionTimestamp> {
        *self.last_finalized_timestamp.read().unwrap()
    }

    pub async fn insert_block(&self, rpc_provider: Arc<RpcProvider>, block: &Block) -> Result<()> {
        let block_resolution_timestamp =
            ResolutionTimestamp::from_timestamp(block.block_timestamp, &self.resolution);

        let last_finalized_timestamp = self.last_finalized_timestamp.read().unwrap().clone();
        let finalize_timestamp_range = get_timestamp_range_to_finalize(
            rpc_provider,
            &block_resolution_timestamp,
            &last_finalized_timestamp,
            &self.resolution,
        )
        .await;

        let block_price_bars = block.uniswap_v2_pairs.iter().fold(
            FnvHashMap::with_capacity_and_hasher(block.uniswap_v2_pairs.len(), Default::default()),
            |mut acc, (pair_address, pair)| {
                match BlockPriceBar::from_uniswap_v2_pair(pair) {
                    Some(block_price_bar) => acc.insert(*pair_address, block_price_bar),
                    None => {
                        log::warn!(
                            block_number = block.block_number;
                            "Failed to create block_price_bar for pair {}", pair_address
                        );

                        None
                    }
                };

                acc
            },
        );

        // Insert the new block price bars
        {
            let mut time_price_bars = self.time_price_bars.write().unwrap();

            // If this block is behind the last inserted block number, this is a reorg and we need to
            // prune existing data
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
                    }
                    _ => {}
                }
            }

            // Insert new BlockPriceBar items into time_price_bars
            for (pair_address, block_price_bar) in block_price_bars.iter() {
                let time_price_bars = time_price_bars
                    .entry(pair_address.clone())
                    .or_insert_with(|| TimePriceBars::new(self.retention_count));

                time_price_bars
                    .insert_data(
                        block_resolution_timestamp.clone(),
                        block.block_number,
                        block_price_bar.clone().into(),
                    )
                    .wrap_err_with(|| {
                        format!(
                            "Failed to insert new block price bar for pair {}",
                            pair_address
                        )
                    })?;
            }

            // Perform maintenance on all price time bars to account for the newly inserted block
            let mut stale_pair_addresses = Vec::new();
            for (pair_address, pair_time_price_bars) in time_price_bars.iter_mut() {
                // Carry forward previous BlockPriceBar items for any pairs without new items in
                // this block
                if !block_price_bars.contains_key(pair_address) {
                    pair_time_price_bars
                        .pad_for_block(&block.block_number, &block_resolution_timestamp)
                        .wrap_err_with(|| {
                            format!(
                                "pad_time_price_bars_for_block failed for pair {}",
                                pair_address
                            )
                        })?
                }

                // Prune the time price bars if they have not had a non-padded bar inserted
                // recently (i.e. they're a dead pair)
                if pair_time_price_bars.is_stale() {
                    stale_pair_addresses.push(pair_address.clone());
                }

                // Finalize any time price bars in finalize range
                if let Some((start_timestamp, end_timestamp)) = &finalize_timestamp_range {
                    pair_time_price_bars.finalize_range(start_timestamp, end_timestamp)?;
                }
            }

            // Prune any stale pair addresses
            for pair_address in stale_pair_addresses.into_iter() {
                time_price_bars.remove(&pair_address);
            }
        }

        // Update the last finalized timestamp if required
        if let Some((_, end_timestamp)) = &finalize_timestamp_range {
            let mut last_finalized_timestamp = self.last_finalized_timestamp.write().unwrap();
            *last_finalized_timestamp = Some(end_timestamp.clone());
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
    use super::{super::Block, get_timestamp_range_to_finalize, TimePriceBarStore};
    use crate::{
        abi::IUniswapV2Pair,
        config,
        indexer::{Resolution, ResolutionTimestamp},
        rpc_provider::{RpcProvider, TTLCache},
    };

    use alloy::{
        primitives::{address, uint, BlockNumber},
        rpc::types::eth::Filter,
        sol_types::SolEvent,
    };

    use eyre::{OptionExt, Result};
    use std::sync::Arc;

    async fn get_block(rpc_provider: Arc<RpcProvider>, block_number: BlockNumber) -> Result<Block> {
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

        Block::parse(rpc_provider, &header, &logs).await
    }

    #[tokio::test]
    async fn test_get_timestamp_range_to_finalize() -> Result<()> {
        let mock_finalized_timestamp = uint!(1000_U256);
        let rpc_provider = {
            let inner = RpcProvider::new(&config::RPC_URL).await?;
            let mock_finalized_block_number = 12822402;
            let mut mock_finalized_header = inner
                .get_block_header(mock_finalized_block_number)
                .await?
                .expect("Expected block header, but found None");

            mock_finalized_header.timestamp = mock_finalized_timestamp.clone();

            Arc::new(
                RpcProvider::new_with_cache(
                    &config::RPC_URL,
                    TTLCache::new(mock_finalized_header, None),
                )
                .await?,
            )
        };

        let initial_finalize_range = get_timestamp_range_to_finalize(
            Arc::clone(&rpc_provider),
            &ResolutionTimestamp::zero(),
            &None,
            &Resolution::FiveMinutes,
        )
        .await
        .expect("Expected initial_finalize_range, but found None");

        assert_eq!(
            initial_finalize_range,
            (
                ResolutionTimestamp::zero(),
                ResolutionTimestamp::from_timestamp(
                    mock_finalized_timestamp.to::<u64>(),
                    &Resolution::FiveMinutes
                )
            )
        );

        let last_finalized_timestamp =
            ResolutionTimestamp::from_timestamp(600_u64, &Resolution::FiveMinutes);
        let finalize_range = get_timestamp_range_to_finalize(
            Arc::clone(&rpc_provider),
            &ResolutionTimestamp::zero(),
            &Some(last_finalized_timestamp),
            &Resolution::FiveMinutes,
        )
        .await
        .expect("Expected finalize_range, but found None");

        assert_eq!(
            finalize_range,
            (
                last_finalized_timestamp.next(&Resolution::FiveMinutes),
                ResolutionTimestamp::from_timestamp(
                    mock_finalized_timestamp.to::<u64>(),
                    &Resolution::FiveMinutes
                )
            )
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_block() -> Result<()> {
        let mock_finalized_timestamp = uint!(1000_U256);
        let rpc_provider = {
            let inner = RpcProvider::new(&config::RPC_URL).await?;
            let mock_finalized_block_number = 12822402;
            let mut mock_finalized_header = inner
                .get_block_header(mock_finalized_block_number)
                .await?
                .expect("Expected block header, but found None");

            mock_finalized_header.timestamp = mock_finalized_timestamp.clone();

            Arc::new(
                RpcProvider::new_with_cache(
                    &config::RPC_URL,
                    TTLCache::new(mock_finalized_header, None),
                )
                .await?,
            )
        };

        // test base case
        {
            let store = TimePriceBarStore::new(Resolution::FiveMinutes, 5);
            let block = get_block(Arc::clone(&rpc_provider), 12822402).await?;
            store
                .insert_block(Arc::clone(&rpc_provider), &block)
                .await?;

            assert_eq!(store.last_inserted_block_number(), Some(block.block_number));
            assert_eq!(
                store.last_finalized_timestamp(),
                Some(ResolutionTimestamp::from_timestamp(
                    mock_finalized_timestamp.to::<u64>(),
                    &Resolution::FiveMinutes
                ))
            );
            assert_eq!(
                store
                    .time_price_bars()
                    .read()
                    .unwrap()
                    .get(&address!("c1c52be5c93429be50f5518a582f690d0fc0528a"))
                    .unwrap()
                    .get_data_range(
                        &ResolutionTimestamp::zero(),
                        &ResolutionTimestamp::from_timestamp(
                            block.block_timestamp,
                            &Resolution::FiveMinutes
                        )
                    )?
                    .len(),
                1
            );
        }

        // test reorg handling
        {
            let store = TimePriceBarStore::new(Resolution::FiveMinutes, 5);
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
