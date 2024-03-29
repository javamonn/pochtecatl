use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    ops::Deref,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{abi::IUniswapV2Pair, config};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockNumber, Bytes},
    pubsub::PubSubFrontend,
    rpc::types::eth::{BlockNumberOrTag, Filter, Header, Log, TransactionRequest},
    sol_types::SolCall,
    transports::TransportResult,
};
use alloy_provider::{Provider, RootProvider};
use alloy_rpc_client::{RpcClient, WsConnect};

use eyre::{eyre, Result, WrapErr};
use lru::LruCache;
use std::sync::RwLock;
use tokio::task::JoinSet;

struct TTLCache<V> {
    value: V,
    ttl: Option<std::time::Duration>,
}

impl<V> TTLCache<V> {
    pub fn new(value: V, ttl: Option<std::time::Duration>) -> Self {
        Self { value, ttl }
    }

    pub fn is_expired(&self) -> bool {
        match self.ttl {
            None => false,
            Some(ttl) => {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    < ttl
            }
        }
    }
}

pub struct RpcProvider {
    rpc_provider: RootProvider<Ethereum, PubSubFrontend>,

    // caches
    uniswap_v2_pair_token_addresses_cache: RwLock<LruCache<Address, (Address, Address)>>,
    finalized_block_header_cache: RwLock<Option<TTLCache<Header>>>,
}

impl RpcProvider {
    pub async fn new(url: &String) -> Result<Self> {
        let rpc_client = RpcClient::connect_pubsub(WsConnect::new(url)).await?;

        Ok(Self {
            rpc_provider: RootProvider::<Ethereum, _>::new(rpc_client),
            uniswap_v2_pair_token_addresses_cache: RwLock::new(LruCache::new(
                NonZeroUsize::new(1000).unwrap(),
            )),
            finalized_block_header_cache: RwLock::new(None),
        })
    }

    // eth api
    pub async fn get_logs(&self, filter: &Filter) -> TransportResult<Vec<Log>> {
        self.rpc_provider.get_logs(filter).await
    }

    // custom api
    pub async fn get_finalized_block_header(&self) -> Result<Header> {
        // Return value from cache if it exists
        {
            if let Some(finalized_block_header) =
                self.finalized_block_header_cache.read().unwrap().deref()
            {
                if !finalized_block_header.is_expired() {
                    return Ok(finalized_block_header.value.clone());
                }
            }
        }

        // Otherwise fetch from rpc and update cache
        let block = self
            .rpc_provider
            .get_block_by_number(BlockNumberOrTag::Finalized, false)
            .await
            .wrap_err("get_block_by_number by failed")
            .and_then(|block| block.ok_or(eyre!("get_block_by_number finalized returned None")))?;

        let cache_ttl = if *config::IS_BACKFILL {
            None
        } else {
            Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    + std::time::Duration::from_secs(300),
            )
        };

        let result = block.header.clone();
        {
            let mut write_cache = self.finalized_block_header_cache.write().unwrap();
            *write_cache = Some(TTLCache::new(block.header, cache_ttl));
        }

        Ok(result)
    }

    pub async fn get_uniswap_v2_pair_token_addresses(
        &self,
        pair_address: Address,
    ) -> Result<(Address, Address)> {
        // Return value from cache if it exists
        {
            let mut write_cache = self.uniswap_v2_pair_token_addresses_cache.write().unwrap();
            if let Some(pair_token_addresses) = write_cache.get(&pair_address) {
                return Ok(pair_token_addresses.clone());
            }
        }

        // Otherwise fetch from rpc and update cache once complete
        let token0_tx = TransactionRequest {
            to: Some(pair_address),
            input: Some(Bytes::from(IUniswapV2Pair::token0Call {}.abi_encode())).into(),
            ..Default::default()
        };
        let token1_tx = TransactionRequest {
            to: Some(pair_address),
            input: Some(Bytes::from(IUniswapV2Pair::token1Call {}.abi_encode())).into(),
            ..Default::default()
        };

        let (token0, token1) = tokio::join!(
            self.rpc_provider.call(&token0_tx, None),
            self.rpc_provider.call(&token1_tx, None)
        );

        let token_addresses = {
            let token0_address = token0
                .wrap_err("token0 call failed")
                .and_then(|result| {
                    IUniswapV2Pair::token0Call::abi_decode_returns(
                        result.as_ref(),
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to abi decode token0")
                })
                .map(|r| r._0)?;
            let token1_address = token1
                .wrap_err("token1 call failed")
                .and_then(|result| {
                    IUniswapV2Pair::token1Call::abi_decode_returns(
                        result.as_ref(),
                        cfg!(debug_assertions),
                    )
                    .wrap_err("failed to abi decode token1")
                })
                .map(|r| r._0)?;
            (token0_address, token1_address)
        };

        // Cache the token addresses
        {
            let mut write_cache = self.uniswap_v2_pair_token_addresses_cache.write().unwrap();
            write_cache.put(pair_address, token_addresses);
        }

        Ok(token_addresses)
    }

    pub async fn get_block_headers_by_range(
        &self,
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
    ) -> BTreeMap<BlockNumber, Header> {
        let mut tasks = JoinSet::new();
        let mut output = BTreeMap::new();

        for block_number in start_block_number..=end_block_number {
            let rpc_provider = self.rpc_provider.clone();
            tasks.spawn(async move {
                match rpc_provider
                    .get_block_by_number(BlockNumberOrTag::Number(block_number), false)
                    .await
                {
                    Ok(Some(block)) => Some((block_number, block.header)),
                    Ok(None) => {
                        log::warn!("get_block_by_number {} no result", block_number);
                        None
                    }
                    Err(err) => {
                        log::error!("get_block_by_number {} failed: {:?}", block_number, err);
                        None
                    }
                }
            });
        }

        while let Some(header) = tasks.join_next().await {
            match header {
                Ok(Some((block_number, header))) => {
                    output.insert(block_number, header);
                }
                Ok(None) => {}
                Err(err) => {
                    log::error!("join_next error: {:?}", err);
                }
            }
        }

        output
    }
}
