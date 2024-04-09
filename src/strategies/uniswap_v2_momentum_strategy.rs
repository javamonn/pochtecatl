use crate::indexer::{IndexedBlockMessage, TimePriceBarStore};

use eyre::Result;
use std::sync::{mpsc::Receiver, Arc};
use tokio::task::JoinHandle;

pub struct UniswapV2MomentumStrategy {
    exec_handle: Option<JoinHandle<Result<()>>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
}

async fn handle_block(
    indexed_block_message: IndexedBlockMessage,
    time_price_bar_store: Arc<TimePriceBarStore>,
) -> Result<()> {
    Ok(())
}

impl UniswapV2MomentumStrategy {
    pub fn new(time_price_bar_store: Arc<TimePriceBarStore>) -> Self {
        Self {
            exec_handle: None,
            time_price_bar_store,
        }
    }

    pub fn exec(&mut self, indexed_block_message_receiver: Receiver<IndexedBlockMessage>) {
        let time_price_bar_store = Arc::clone(&self.time_price_bar_store);

        let exec_handle = tokio::spawn(async move {
            while let Ok(indexed_block_message) = indexed_block_message_receiver.recv() {
                let time_price_bar_store = Arc::clone(&time_price_bar_store);
                handle_block(indexed_block_message, time_price_bar_store).await?;
            }

            Ok(())
        });

        self.exec_handle = Some(exec_handle);
    }

    pub async fn join(self) -> Result<()> {
        if let Some(exec_handle) = self.exec_handle {
            exec_handle.await??;
        }

        Ok(())
    }
}
