use super::multicall;

use crate::abi::{multicall3, IUniswapV2Pair};

use alloy::{
    network::Ethereum, primitives::Address, providers::Provider, rpc::types::eth::BlockId,
    sol_types::SolCall, transports::Transport,
};

#[cfg(test)]
use alloy::{
    network::TransactionBuilder,
    primitives::{Bytes, U256},
    rpc::types::eth::TransactionRequest,
};

use eyre::{eyre, Result, WrapErr};
use fnv::FnvHashMap;
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use tracing::instrument;

pub struct UniswapV2PairProvider<T: Transport + Clone, P: Provider<T, Ethereum>> {
    token_addresses_cache: Mutex<LruCache<Address, (Address, Address)>>,
    inner: Arc<P>,
    _transport_marker: std::marker::PhantomData<T>,
}

impl<T, P> UniswapV2PairProvider<T, P>
where
    P: Provider<T, Ethereum> + 'static,
    T: Transport + Clone + 'static,
{
    pub fn new(inner: Arc<P>) -> Self {
        Self {
            token_addresses_cache: Mutex::new(LruCache::new(NonZeroUsize::new(1000).unwrap())),
            inner,
            _transport_marker: std::marker::PhantomData,
        }
    }

    #[instrument(skip_all)]
    pub async fn get_token_addresses(
        &self,
        pair_addresses: Vec<Address>,
        block_id: Option<BlockId>,
    ) -> Result<FnvHashMap<Address, (Address, Address)>> {
        let mut output =
            FnvHashMap::with_capacity_and_hasher(pair_addresses.len(), Default::default());

        let mut calls = Vec::with_capacity(pair_addresses.len() * 2);
        let mut call_pair_addresses = Vec::with_capacity(pair_addresses.len());

        {
            let mut token_addresses_cache = self.token_addresses_cache.lock().unwrap();
            for pair_address in pair_addresses.iter() {
                if let Some(token_addresses) = token_addresses_cache.get(pair_address) {
                    output.insert(pair_address.clone(), token_addresses.clone());
                } else {
                    call_pair_addresses.push(pair_address.clone());
                    calls.push(multicall3::Call3 {
                        target: pair_address.clone(),
                        allowFailure: true,
                        callData: IUniswapV2Pair::token0Call {}.abi_encode().into(),
                    });
                    calls.push(multicall3::Call3 {
                        target: pair_address.clone(),
                        allowFailure: true,
                        callData: IUniswapV2Pair::token1Call {}.abi_encode().into(),
                    });
                }
            }
        }

        let res = multicall(Arc::clone(&self.inner), calls, block_id).await?;
        {
            let mut token_addresses_cache = self.token_addresses_cache.lock().unwrap();
            res.chunks_exact(2)
                .enumerate()
                .for_each(|(call_address_idx, res)| {
                    let token0_returns = if res[0].success {
                        IUniswapV2Pair::token0Call::abi_decode_returns(
                            &res[0].returnData,
                            cfg!(debug_assertions),
                        )
                        .wrap_err("failed to decode token0 call returns")
                    } else {
                        Err(eyre!("token0 call error"))
                    };

                    let token1_returns = if res[1].success {
                        IUniswapV2Pair::token1Call::abi_decode_returns(
                            &res[1].returnData,
                            cfg!(debug_assertions),
                        )
                        .wrap_err("failed to decode token1 call returns")
                    } else {
                        Err(eyre!("token1 call error"))
                    };

                    match (
                        call_pair_addresses.get(call_address_idx),
                        token0_returns,
                        token1_returns,
                    ) {
                        (Some(pair_address), Ok(token0_res), Ok(token1_res)) => {
                            token_addresses_cache
                                .put(pair_address.clone(), (token0_res._0, token1_res._0));
                            output.insert(pair_address.clone(), (token0_res._0, token1_res._0));
                        }
                        _ => { /* noop: ignore call errors, address may not be IUniswapV2Pair */ }
                    }
                });
        }

        Ok(output)
    }

    #[cfg(test)]
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

#[cfg(test)]
mod tests {
    use crate::{config, providers::rpc_provider::new_http_signer_provider};

    use alloy::primitives::address;

    use eyre::Result;

    #[tokio::test]
    pub async fn test_get_token_addresses() -> Result<()> {
        let rpc_provider = new_http_signer_provider(&config::RPC_URL, None).await?;

        let res = rpc_provider
            .uniswap_v2_pair_provider()
            .get_token_addresses(
                vec![
                    address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA"),
                    address!("3c6554c1EF9845d629d333A24Ef1b13fCbC89577"),
                ],
                None,
            )
            .await?;

        assert_eq!(res.len(), 2);
        assert_eq!(
            res.get(&address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA")),
            Some(&(
                address!("4200000000000000000000000000000000000006"),
                address!("9a26F5433671751C3276a065f57e5a02D2817973")
            ))
        );
        assert_eq!(
            res.get(&address!("3c6554c1EF9845d629d333A24Ef1b13fCbC89577")),
            Some(&(
                address!("4200000000000000000000000000000000000006"),
                address!("5e9fE073Df7Ce50E91EB9CBb010B99EF6035a97D")
            ))
        );

        Ok(())
    }
}
