use super::Indexer;
use crate::primitives::IndexedBlock;

use alloy::{
    network::Ethereum, primitives::BlockNumber, pubsub::PubSubFrontend, rpc::types::eth::Filter,
};
use alloy_provider::{Provider, RootProvider};

use eyre::Result;
use std::{
    cmp::min,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc,
    },
};
use tokio::task::JoinHandle;

pub struct BlockRangeIndexer {
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
    index_handle: Option<JoinHandle<Result<()>>>,
}

impl BlockRangeIndexer {
    pub fn new(
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
    ) -> BlockRangeIndexer {
        BlockRangeIndexer {
            start_block_number,
            end_block_number,
            index_handle: None,
        }
    }
}

const BLOCK_RANGE_STEP_BY: u64 = 100;

async fn index(
    rpc_provider: Arc<RootProvider<Ethereum, PubSubFrontend>>,
    indexed_block_sender: SyncSender<IndexedBlock>,
    start_block_number: BlockNumber,
    end_block_number: BlockNumber,
) -> Result<()> {
    for range_start_block_number in
        (start_block_number..end_block_number).step_by(BLOCK_RANGE_STEP_BY as usize)
    {
        let range_end_block_number = min(
            range_start_block_number + BLOCK_RANGE_STEP_BY,
            end_block_number,
        );

        let filter = Filter::new()
            .from_block(range_start_block_number)
            .to_block(range_end_block_number);

        let logs = rpc_provider.get_logs(&filter).await;
    }

    Ok(())
}

impl Indexer for BlockRangeIndexer {
    fn subscribe(
        &mut self,
        rpc_provider: &Arc<RootProvider<Ethereum, PubSubFrontend>>,
    ) -> Receiver<IndexedBlock> {
        let (indexed_block_sender, indexed_block_receiver) = sync_channel(64);

        let start_block_number = self.start_block_number;
        let end_block_number = self.end_block_number;
        let rpc_provider = Arc::clone(rpc_provider);
        let index_handle = tokio::spawn(async move {
            index(
                rpc_provider,
                indexed_block_sender,
                start_block_number,
                end_block_number,
            )
            .await
        });

        self.index_handle = Some(index_handle);

        indexed_block_receiver
    }
}
