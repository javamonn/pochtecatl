use crate::{abi::IUniswapV2Pair, config};

use alloy::{
    network::{Ethereum, EthereumSigner, TransactionBuilder},
    primitives::{Address, BlockNumber, Bytes, U256},
    providers::{
        layers::{GasEstimatorProvider, ManagedNonceProvider, SignerProvider},
        Provider, ProviderBuilder, RootProvider,
    },
    pubsub::PubSubFrontend,
    rpc::{
        client::WsConnect,
        types::{
            eth::{
                BlockId, BlockNumberOrTag, Filter, Header, Log, TransactionReceipt,
                TransactionRequest,
            },
            trace::parity::{TraceResults, TraceType},
        },
    },
    signers::wallet::LocalWallet,
    sol_types::SolCall,
    transports::TransportResult,
};

use eyre::{eyre, Result, WrapErr};
use lru::LruCache;
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    ops::Deref,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::task::JoinSet;

pub struct TTLCache<V> {
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

type AlloyRpcProvider = SignerProvider<
    PubSubFrontend,
    GasEstimatorProvider<
        PubSubFrontend,
        ManagedNonceProvider<PubSubFrontend, RootProvider<PubSubFrontend>>,
        Ethereum,
    >,
    EthereumSigner,
>;

pub struct RpcProvider {
    signer_address: Address,
    rpc_provider: Arc<AlloyRpcProvider>,

    // caches
    uniswap_v2_pair_token_addresses_cache: RwLock<LruCache<Address, (Address, Address)>>,
    finalized_block_header_cache: RwLock<Option<TTLCache<Header>>>,
}

async fn make_provider(url: &String) -> Result<(AlloyRpcProvider, Address)> {
    let signer = LocalWallet::from_bytes(&config::WALLET_PRIVATE_KEY)?;

    let signer_address = signer.address();

    let provider = ProviderBuilder::new()
        .signer(EthereumSigner::from(signer))
        .with_gas_estimation()
        .with_nonce_management()
        .on_ws(WsConnect::new(url))
        .await
        .map_err(|err| eyre!("Failed to create provider: {:?}", err))?;

    Ok((provider, signer_address))
}

impl RpcProvider {
    pub async fn new(url: &String) -> Result<Self> {
        Self::new_with_cache(url, None).await
    }

    pub async fn new_with_cache(
        url: &String,
        finalized_block_header_cache: Option<TTLCache<Header>>,
    ) -> Result<Self> {
        let (provider, signer_address) = make_provider(url).await?;

        Ok(Self {
            rpc_provider: Arc::new(provider),
            signer_address,
            uniswap_v2_pair_token_addresses_cache: RwLock::new(LruCache::new(
                NonZeroUsize::new(1000).unwrap(),
            )),
            finalized_block_header_cache: RwLock::new(finalized_block_header_cache),
        })
    }

    pub async fn trace_call_many(
        &self,
        tx_requests: &[(TransactionRequest, Vec<TraceType>)],
        block_id: Option<BlockId>,
    ) -> Result<TraceResults> {
        self.rpc_provider
            .trace_call_many(tx_requests, block_id)
            .await
            .wrap_err("trace_call_many failed")
    }

    // eth api
    pub async fn send_transaction(
        &self,
        tx_request: TransactionRequest,
    ) -> Result<TransactionReceipt> {
        self.rpc_provider
            .send_transaction(tx_request)
            .await?
            .with_timeout(Some(Duration::from_secs(10)))
            .with_required_confirmations(1)
            .get_receipt()
            .await
            .wrap_err("send_transaction failed")
    }

    pub async fn get_logs(&self, filter: &Filter) -> TransportResult<Vec<Log>> {
        self.rpc_provider.get_logs(filter).await
    }

    // custom api
    pub fn signer_address(&self) -> Address {
        self.signer_address
    }

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

    pub async fn get_uniswap_v2_pair_reserves(
        &self,
        pair_address: Address,
        block_id: Option<BlockId>,
    ) -> Result<(U256, U256)> {
        let tx = TransactionRequest::default()
            .with_to(pair_address.into())
            .with_input(Bytes::from(IUniswapV2Pair::getReservesCall {}.abi_encode()));

        let reserves = self
            .rpc_provider
            .call(&tx, block_id)
            .await
            .wrap_err("get_uniswap_v2_pair_reserves call failed")
            .and_then(|result| {
                println!("{:?}", pair_address);
                println!("{:?}", result);
                IUniswapV2Pair::getReservesCall::abi_decode_returns(
                    result.as_ref(),
                    cfg!(debug_assertions),
                )
                .wrap_err("failed to abi decode getReserves")
            })?;

        Ok((U256::from(reserves.reserve0), U256::from(reserves.reserve1)))
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

    #[cfg(test)]
    pub async fn get_block_header(&self, block_number: BlockNumber) -> Result<Option<Header>> {
        self.rpc_provider
            .get_block_by_number(block_number.into(), false)
            .await
            .wrap_err_with(|| format!("get_block_by_number {} failed", block_number))
            .map(|block| block.map(|block| block.header))
    }

    pub async fn get_block_headers_by_range(
        &self,
        start_block_number: BlockNumber,
        end_block_number: BlockNumber,
    ) -> BTreeMap<BlockNumber, Header> {
        let mut tasks = JoinSet::new();
        let mut output = BTreeMap::new();

        for block_number in start_block_number..=end_block_number {
            let rpc_provider = Arc::clone(&self.rpc_provider);
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
