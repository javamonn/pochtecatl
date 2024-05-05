use super::AsyncLruCache;

use crate::abi::IUniswapV2Pair;

use alloy::{
    network::{Ethereum, TransactionBuilder},
    primitives::{Address, Bytes, U256},
    providers::Provider,
    rpc::types::eth::{BlockId, TransactionRequest},
    sol_types::SolCall,
    transports::Transport,
};

use eyre::{eyre, Result, WrapErr};
use std::{num::NonZeroUsize, sync::Arc};

pub struct UniswapV2PairProvider<T: Transport + Clone, P: Provider<T, Ethereum>> {
    token_addresses_cache: AsyncLruCache<Address, (Address, Address), Arc<P>>,
    inner: Arc<P>,
    _transport_marker: std::marker::PhantomData<T>,
}

impl<T, P> UniswapV2PairProvider<T, P>
where
    P: Provider<T, Ethereum> + 'static,
    T: Transport + Clone + 'static,
{
    pub fn new(inner: Arc<P>) -> Self {
        let token_addresses_cache = AsyncLruCache::new(
            NonZeroUsize::new(1000).unwrap(),
            Box::new(|pair_address, rpc_provider| {
                Box::pin(get_uniswap_v2_pair_token_addresses(
                    rpc_provider,
                    pair_address,
                ))
            }),
        );

        Self {
            token_addresses_cache,
            inner,
            _transport_marker: std::marker::PhantomData,
        }
    }

    pub async fn get_uniswap_v2_pair_token_addresses(
        &self,
        pair_address: Address,
    ) -> Result<(Address, Address)> {
        self.token_addresses_cache
            .get_or_resolve(&pair_address, Arc::clone(&self.inner))
            .await
            .map_err(|err| eyre!("get_uniswap_v2_pair_token_addresses failed: {:?}", err))
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
            .inner
            .call(&tx, block_id)
            .await
            .wrap_err("get_uniswap_v2_pair_reserves call failed")
            .and_then(|result| {
                IUniswapV2Pair::getReservesCall::abi_decode_returns(
                    result.as_ref(),
                    cfg!(debug_assertions),
                )
                .wrap_err("failed to abi decode getReserves")
            })?;

        Ok((U256::from(reserves.reserve0), U256::from(reserves.reserve1)))
    }
}

pub async fn get_uniswap_v2_pair_token_addresses<P, T>(
    rpc_provider: Arc<P>,
    pair_address: Address,
) -> Result<(Address, Address)>
where
    P: Provider<T, Ethereum>,
    T: Transport + Clone,
{
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
        rpc_provider.call(&token0_tx, None),
        rpc_provider.call(&token1_tx, None)
    );

    let token0_address = token0
        .wrap_err("token0 call failed")
        .and_then(|result| {
            IUniswapV2Pair::token0Call::abi_decode_returns(result.as_ref(), cfg!(debug_assertions))
                .wrap_err("failed to abi decode token0")
        })
        .map(|r| r._0)?;
    let token1_address = token1
        .wrap_err("token1 call failed")
        .and_then(|result| {
            IUniswapV2Pair::token1Call::abi_decode_returns(result.as_ref(), cfg!(debug_assertions))
                .wrap_err("failed to abi decode token1")
        })
        .map(|r| r._0)?;

    Ok((token0_address, token1_address))
}
