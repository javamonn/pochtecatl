use super::{
    log_parser::Block, rpc_utils::get_block_uniswap_v2_pair_token_addresses, time_price_bars,
    BlockPriceBar, Resolution, ResolutionTimestamp, TimePriceBars,
};
use crate::{config, rpc_provider::RpcProvider};

use alloy::primitives::{Address, BlockNumber};

use eyre::{Context, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};

async fn get_block_price_bars(
    rpc_provider: Arc<RpcProvider>,
    block: &Block,
) -> FnvHashMap<Address, BlockPriceBar> {
    let uniswap_v2_pair_token_addresses =
        get_block_uniswap_v2_pair_token_addresses(rpc_provider, block).await;

    // Parse uniswap v2 trades into block price bars
    block.uniswap_v2_trades.iter().fold(
        FnvHashMap::with_capacity_and_hasher(block.uniswap_v2_trades.len(), Default::default()),
        |mut acc, (pair_address, trades)| {
            match uniswap_v2_pair_token_addresses.get(pair_address) {
                Some((token0_address, token1_address)) => {
                    if let Some(block_price_bar) = BlockPriceBar::from_uniswap_v2_trades(
                        trades,
                        token0_address,
                        token1_address,
                    ) {
                        acc.insert(*pair_address, block_price_bar);
                    }
                }
                None => {
                    log::warn!(
                        "Expected pair_token_addresses for {}, but found None",
                        pair_address
                    );
                }
            };

            acc
        },
    )
}

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

// Insert new block price bars for the block
fn insert_block_price_bars(
    time_price_bars: &mut FnvHashMap<Address, TimePriceBars>,
    block: &Block,
    block_resolution_timestamp: &ResolutionTimestamp,
    block_price_bars: &FnvHashMap<Address, BlockPriceBar>,
    retention_count: &u64,
) -> Result<()> {
    for (pair_address, block_price_bar) in block_price_bars.iter() {
        let time_price_bars = time_price_bars
            .entry(pair_address.clone())
            .or_insert_with(|| TimePriceBars::new(retention_count.clone()));

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

    Ok(())
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

    pub async fn insert_block(&self, rpc_provider: Arc<RpcProvider>, block: &Block) -> Result<()> {
        let block_resolution_timestamp =
            ResolutionTimestamp::from_timestamp(block.block_timestamp, &self.resolution);

        let (block_price_bars, finalize_timestamp_range) = {
            let last_finalized_timestamp = self.last_finalized_timestamp.read().unwrap().clone();

            tokio::join!(
                get_block_price_bars(Arc::clone(&rpc_provider), block),
                get_timestamp_range_to_finalize(
                    rpc_provider,
                    &block_resolution_timestamp,
                    &last_finalized_timestamp,
                    &self.resolution
                )
            )
        };

        // TODO: need to handle pruning "stale" pairs without any non-carried time price bars in X
        // blocks

        // Insert the new block price bars
        {
            let mut time_price_bars = self.time_price_bars.write().unwrap();

            // If this block is behind the last inserted block number, this is a reorg and we need to
            // prune existing data
            {
                let last_inserted_block_number = self.last_inserted_block_number.read().unwrap();

                match *last_inserted_block_number {
                    Some(last_inserted_block_number)
                        if block.block_number < last_inserted_block_number =>
                    {
                        for pair_time_price_bars in time_price_bars.values_mut() {
                            pair_time_price_bars
                                .prune_to_reorged_block_number(block.block_number)?;
                        }
                    }
                    _ => {}
                }
            }

            // Insert new BlockPriceBar items into time_price_bars
            insert_block_price_bars(
                &mut time_price_bars,
                block,
                &block_resolution_timestamp,
                &block_price_bars,
                &self.retention_count,
            )?;

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
