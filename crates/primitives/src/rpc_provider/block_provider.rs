use super::TTLCache;

use alloy::{
    network::Ethereum,
    primitives::BlockNumber,
    providers::Provider,
    rpc::types::eth::{Block, BlockNumberOrTag, Header},
    transports::Transport,
};

use eyre::{eyre, Result, WrapErr};
use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::instrument;

const BLOCK_CACHE_SIZE: usize = 100;

pub struct BlockProvider<T, P>
where
    P: Provider<T, Ethereum>,
    T: Transport + Clone,
{
    inner: Arc<P>,
    block_cache: Arc<RwLock<BTreeMap<BlockNumber, Block>>>,
    finalized_block_header_cache: RwLock<Option<TTLCache<Header>>>,
    is_backtest: bool,
    _transport_marker: std::marker::PhantomData<T>,
}

impl<T, P> BlockProvider<T, P>
where
    P: Provider<T, Ethereum> + 'static,
    T: Transport + Clone + 'static,
{
    pub fn new(
        inner: Arc<P>,
        finalized_block_header_cache: Option<TTLCache<Header>>,
        is_backtest: bool,
    ) -> Self {
        Self {
            inner,
            is_backtest,
            block_cache: Arc::new(RwLock::new(BTreeMap::new())),
            finalized_block_header_cache: RwLock::new(finalized_block_header_cache),
            _transport_marker: std::marker::PhantomData,
        }
    }

    pub async fn get_block(&self, block_number: BlockNumber) -> Result<Option<Block>> {
        // Return value from cache if it exists
        {
            let read_cache = self.block_cache.read().unwrap();
            if let Some(block) = read_cache.get(&block_number) {
                return Ok(Some(block.clone()));
            }
        }

        // Otherwise fetch from rpc and update cache once complete
        let block = self
            .inner
            .get_block_by_number(block_number.into(), false)
            .await
            .wrap_err_with(|| format!("get_block_by_number {} failed", block_number))?;

        if let Some(block) = block.clone() {
            let mut write_cache = self.block_cache.write().unwrap();
            write_cache.insert(block_number, block.clone());

            // Trim cache if required
            while write_cache.len() > BLOCK_CACHE_SIZE + (BLOCK_CACHE_SIZE / 2) {
                write_cache.pop_first();
            }
        }

        Ok(block)
    }

    #[instrument(skip(self))]
    pub async fn get_block_header(&self, block_number: BlockNumber) -> Result<Option<Header>> {
        self.get_block(block_number)
            .await
            .map(|block| block.map(|block| block.header))
    }

    pub async fn get_finalized_block_header(&self) -> Result<Header> {
        // Return value from cache if it exists
        {
            if let Some(finalized_block_header) =
                self.finalized_block_header_cache.read().unwrap().deref()
            {
                if !finalized_block_header.is_expired() {
                    return Ok(finalized_block_header.value().clone());
                }
            }
        }

        // Otherwise fetch from rpc and update cache
        let header = self
            .inner
            .get_block_by_number(BlockNumberOrTag::Finalized, false)
            .await
            .wrap_err("Failed to get finalized block")
            .and_then(|block| match block {
                Some(block) => Ok(block.header),
                None => Err(eyre!("No finalized block header")),
            })?;

        {
            let mut write_cache = self.finalized_block_header_cache.write().unwrap();
            *write_cache = Some(TTLCache::new(
                header.clone(),
                if self.is_backtest {
                    None
                } else {
                    Some(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("Time went backwards")
                            + std::time::Duration::from_secs(300),
                    )
                },
            ));
        }

        Ok(header)
    }
}
