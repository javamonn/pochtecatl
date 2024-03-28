use alloy::{network::Ethereum, pubsub::PubSubFrontend};
use alloy_provider::RootProvider;

use crate::primitives::IndexedBlock;
use std::sync::{mpsc::Receiver, Arc};

pub trait Indexer {
    fn subscribe(
        &mut self,
        rpc_provider: &Arc<RootProvider<Ethereum, PubSubFrontend>>,
    ) -> Receiver<IndexedBlock>;
}
