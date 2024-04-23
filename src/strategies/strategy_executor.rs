use crate::indexer::IndexedBlockMessage;

use std::sync::mpsc::Receiver;
use eyre::Result;

pub trait StrategyExecutor {
    fn exec(&mut self, indexed_block_message_receiver: Receiver<IndexedBlockMessage>);
    async fn join(self) -> Result<()>;
}
