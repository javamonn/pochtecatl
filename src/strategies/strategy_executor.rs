use crate::indexer::{IndexedBlockMessage, TimePriceBarStore};

use eyre::Result;

pub trait StrategyExecutor {
    fn on_indexed_block_message(
        &self,
        indexed_block_message: IndexedBlockMessage,
        time_price_bar_store: &TimePriceBarStore,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}
