mod config;
mod indexer;
mod primitives;

use indexer::BlockRangeIndexer;
use primitives::BlockId;

use alloy::{network::Ethereum, pubsub::PubSubFrontend};
use alloy_provider::RootProvider;
use alloy_rpc_client::{RpcClient, WsConnect};

use eyre::{eyre, Result};
use std::sync::Arc;

fn make_price_indexer(
    start_block_id: BlockId,
    end_block_id: BlockId,
) -> Result<Box<dyn indexer::Indexer>> {
    match (start_block_id, end_block_id) {
        (BlockId::BlockNumber(start), BlockId::BlockNumber(end)) => {
            if start < end {
                Ok(Box::new(BlockRangeIndexer::new(start, end)))
            } else {
                Err(eyre!(
                    "Failed to create BlockRangeIndexer due to invalid block numbers: start {}, end {}",
                    start,
                    end
                ))
            }
        }
        (BlockId::Latest, BlockId::Latest) => unimplemented!(),
        _ => Err(eyre!("Failed to create Indexer")),
    }
}

async fn make_rpc_provider() -> Result<RootProvider<Ethereum, PubSubFrontend>> {
    let rpc_client = RpcClient::connect_pubsub(WsConnect::new(config::rpc_url())).await?;
    Ok(RootProvider::<Ethereum, _>::new(rpc_client))
}

#[tokio::main]
async fn main() -> Result<()> {
    structured_logger::Builder::with_level(&config::rust_log()).init();

    let start_block_id = config::start_block_id();
    let end_block_id = config::end_block_id();

    log::info!(
        rust_log = config::rust_log(),
        start_block_id = start_block_id,
        end_block_id = end_block_id;
        "start"
    );

    let rpc_provider = Arc::new(make_rpc_provider().await?);
    let mut price_indexer = make_price_indexer(start_block_id, end_block_id)?;

    let indexed_block_receiver = price_indexer.subscribe(&rpc_provider);

    Ok(())
}
