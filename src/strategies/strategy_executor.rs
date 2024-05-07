use crate::indexer::IndexedBlockMessage;

use eyre::Result;
use tokio::sync::mpsc::Receiver;

pub trait StrategyExecutor {
    fn exec(&mut self, indexed_block_message_receiver: Receiver<IndexedBlockMessage>);
    async fn join(self) -> Result<()>;
}
