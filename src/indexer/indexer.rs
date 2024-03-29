use crate::rpc_provider::RpcProvider;
use alloy::primitives::{BlockHash, BlockNumber};

use std::sync::{mpsc::Receiver, Arc};

pub struct IndexedBlockMetadata {
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
}

pub trait Indexer {
    fn subscribe(&mut self, rpc_provider: &Arc<RpcProvider>) -> Receiver<IndexedBlockMetadata>;
}
