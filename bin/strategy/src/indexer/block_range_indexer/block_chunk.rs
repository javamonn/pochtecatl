use pochtecatl_db::BlockModel;
use pochtecatl_primitives::{constants, Block, BlockBuilder, IndexedTrade, RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::BlockNumber,
    providers::Provider,
    rpc::types::eth::{Filter, Log as RpcLog},
    transports::Transport,
};

use eyre::{eyre, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::collections::BTreeMap;
use tracing::{debug, error};

pub enum BlockChunkSource {
    Rpc,
    Db,
}

pub struct BlockChunk {
    pub data: Vec<Block>,
    pub source: BlockChunkSource,
}

impl BlockChunk {
    pub async fn fetch<T, P>(
        rpc_provider: &RpcProvider<T, P>,
        db_provider: &Pool<SqliteConnectionManager>,
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
    ) -> Result<Self>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        match BlockChunk::fetch_from_db(db_provider, start_block_number, end_block_number) {
            Ok(block_chunk) => {
                debug!(
                    range_start_block_number = start_block_number,
                    range_end_block_number = end_block_number,
                    "Fetched block chunk from db"
                );
                Ok(block_chunk)
            }
            Err(_) => {
                debug!(
                    range_start_block_number = start_block_number,
                    range_end_block_number = end_block_number,
                    "Fetched block chunk from rpc"
                );
                BlockChunk::fetch_from_rpc(rpc_provider, start_block_number, end_block_number).await
            }
        }
    }

    pub async fn fetch_from_rpc<T, P>(
        rpc_provider: &RpcProvider<T, P>,
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
    ) -> Result<Self>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        let filter = Filter::new()
            .from_block(start_block_number)
            .to_block(end_block_number)
            .event_signature(IndexedTrade::event_signature_hashes());

        let (logs, start_block_header) = tokio::join!(
            rpc_provider.get_logs(&filter),
            rpc_provider
                .block_provider()
                .get_block_header(start_block_number)
        );

        let mut logs_by_block_number: BTreeMap<BlockNumber, Vec<RpcLog>> = logs
            .unwrap_or_else(|err| {
                error!(
                    range_start_block_number = start_block_number,
                    range_end_block_number = end_block_number,
                    "get_logs failed: {:?}",
                    err
                );
                Vec::new()
            })
            .into_iter()
            .fold(BTreeMap::new(), |mut acc, log| {
                acc.entry(log.block_number.unwrap())
                    .or_insert_with(Vec::new)
                    .push(log);

                acc
            });

        let start_block_timestamp = start_block_header.and_then(|header| {
            header
                .map(|h| h.timestamp.to::<u64>())
                .ok_or_else(|| eyre!("Expected block {} but found None", start_block_number))
        })?;

        // Parse the blocks
        let block_builders = (start_block_number..=end_block_number)
            .map(|block_number| {
                let block_logs = logs_by_block_number
                    .remove(&block_number)
                    .unwrap_or_default();

                // Estimate the block timestamp from the start block timestamp and average
                // block time. This avoids an rpc call for each block to lookup the header
                // or split the multicall.
                let estimated_block_timestamp = start_block_timestamp
                    + (block_number - start_block_number) * constants::AVERAGE_BLOCK_TIME_SECONDS;

                BlockBuilder::new(block_number, estimated_block_timestamp, &block_logs)
            })
            .collect();

        Ok(BlockChunk {
            data: BlockBuilder::build_many(block_builders, rpc_provider).await?,
            source: BlockChunkSource::Rpc,
        })
    }

    fn fetch_from_db(
        db_pool: &Pool<SqliteConnectionManager>,
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
    ) -> Result<Self> {
        let mut conn = db_pool.get()?;
        let tx = conn.transaction()?;
        let blocks = BlockModel::query_by_number_range(&tx, start_block_number, end_block_number)?;
        if blocks.len() == (end_block_number - start_block_number + 1) as usize {
            Ok(BlockChunk {
                data: blocks
                    .into_iter()
                    .filter_map(|b| b.try_into().ok())
                    .collect(),
                source: BlockChunkSource::Db,
            })
        } else {
            Err(eyre!("Blocks not found in db."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockChunk, BlockChunkSource};
    use crate::config;
    use pochtecatl_db::{connect as connect_db, BlockModel};
    use pochtecatl_primitives::new_http_signer_provider;

    use alloy::primitives::address;

    use eyre::Result;
    use hex_literal::hex;
    use std::sync::Arc;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_fetch_block_chunk_rpc() -> Result<()> {
        let rpc_provider = Arc::new(
            new_http_signer_provider(
                url::Url::parse(config::RPC_URL.as_str())?,
                &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
                None,
                true,
            )
            .await?,
        );
        let db_pool = connect_db(&String::from(":memory:"))?;

        let start_block_number = 13868901;
        let end_block_number = 13868921;

        let (block_chunk, end_block_header) = tokio::join!(
            BlockChunk::fetch(
                &rpc_provider,
                &db_pool,
                start_block_number,
                end_block_number
            ),
            rpc_provider
                .block_provider()
                .get_block_header(end_block_number)
        );

        let block_chunk = block_chunk?;
        let end_block_header = end_block_header?;

        assert_eq!(block_chunk.data.len(), 21);

        let first_block = block_chunk.data.first().unwrap();
        assert_eq!(first_block.block_number, 13868901);
        assert_eq!(first_block.pair_ticks.len(), 19);

        let last_block = block_chunk.data.last().unwrap();
        assert_eq!(last_block.block_number, 13868921);
        assert_eq!(
            last_block.block_timestamp,
            end_block_header.unwrap().timestamp.to::<u64>()
        );

        assert!(matches!(block_chunk.source, BlockChunkSource::Rpc));

        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_block_chunk_db() -> Result<()> {
        let rpc_provider = Arc::new(
            new_http_signer_provider(
                url::Url::parse(config::RPC_URL.as_str())?,
                &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
                None,
                true,
            )
            .await?,
        );
        let db_pool = connect_db(&String::from(":memory:"))?;

        let start_block_number = 1_u8;
        let end_block_number = 20_u8;

        {
            let mut conn = db_pool.get()?;
            let tx = conn.transaction()?;

            for block_number in start_block_number..=end_block_number {
                BlockModel {
                    number: (block_number as u64).into(),
                    timestamp: (block_number as u64).into(),
                    pair_ticks: serde_json::json!({}),
                }
                .insert(&tx)?;
            }

            tx.commit()?;
        }

        let block_chunk = BlockChunk::fetch(
            &rpc_provider,
            &db_pool,
            start_block_number as u64,
            end_block_number as u64,
        )
        .await?;

        assert_eq!(block_chunk.data.len(), 20);
        assert!(block_chunk
            .data
            .first()
            .is_some_and(|b| b.block_number == 1));
        assert!(block_chunk
            .data
            .last()
            .is_some_and(|b| b.block_number == 20));
        assert!(matches!(block_chunk.source, BlockChunkSource::Db));

        Ok(())
    }
}
