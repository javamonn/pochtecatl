use super::multicall::multicall;
use crate::primitives::{Pair, PairInput};

use alloy::{
    network::Ethereum, primitives::Address, providers::Provider, rpc::types::eth::BlockId,
    transports::Transport,
};

use eyre::{Context, Result};
use fnv::FnvHashMap;
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use tokio::task::JoinSet;

struct AsyncValue<T: Clone> {
    value: Option<T>,
    tx: tokio::sync::broadcast::Sender<T>,
}

enum ReceiverOrValue<T: Clone> {
    Receiver(tokio::sync::broadcast::Receiver<T>),
    Value(T),
}

impl<T: Clone> AsyncValue<T> {
    fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(1);
        Self { value: None, tx }
    }

    fn get_receiver_or_value(&self) -> ReceiverOrValue<T> {
        if let Some(value) = &self.value {
            ReceiverOrValue::Value(value.clone())
        } else {
            ReceiverOrValue::Receiver(self.tx.subscribe())
        }
    }

    fn set(&mut self, value: T) {
        self.value = Some(value.clone());

        // we don't care if receiver has been dropped
        let _ = self.tx.send(value);
    }
}

pub struct IndexedTradeProvider<T: Transport + Clone, P: Provider<T, Ethereum>> {
    pair_cache: Mutex<LruCache<Address, AsyncValue<Option<Pair>>>>,
    inner: Arc<P>,
    _transport_marker: std::marker::PhantomData<T>,
}

async fn resolve_pair_metadata_async_values(
    values: FnvHashMap<Address, ReceiverOrValue<Option<Pair>>>,
) -> Result<FnvHashMap<Address, Pair>> {
    let mut output = FnvHashMap::with_capacity_and_hasher(values.len(), Default::default());
    let mut join_set = JoinSet::new();

    for (pair_address, r) in values.into_iter() {
        match r {
            ReceiverOrValue::Value(Some(value)) => {
                output.insert(pair_address, value);
            }
            ReceiverOrValue::Value(None) => { /* noop: ignore none results in output */ }
            ReceiverOrValue::Receiver(mut receiver) => {
                let pair_address = pair_address.clone();
                join_set.spawn(async move {
                    let value = receiver.recv().await;
                    (pair_address, value)
                });
            }
        }
    }

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((pair_address, Ok(Some(value)))) => {
                output.insert(pair_address, value);
            }
            Ok((_, Ok(None))) => { /* noop: ignore none results in output */ }
            Ok((_, Err(e))) => {
                return Err(e).context("Failed to resolve pair metadata async value");
            }
            Err(e) => {
                return Err(e).context("Failed to resolve pair metadata async value");
            }
        }
    }

    Ok(output)
}

impl<T, P> IndexedTradeProvider<T, P>
where
    P: Provider<T, Ethereum> + 'static,
    T: Transport + Clone + 'static,
{
    pub fn new(inner: Arc<P>) -> Self {
        Self {
            pair_cache: Mutex::new(LruCache::new(NonZeroUsize::new(2500).unwrap())),
            inner,
            _transport_marker: std::marker::PhantomData,
        }
    }

    pub async fn get_pairs(
        &self,
        pair_inputs: Vec<PairInput>,
        block_id: Option<BlockId>,
    ) -> Result<FnvHashMap<Address, Pair>> {
        let mut async_values = FnvHashMap::default();
        let mut pair_results = FnvHashMap::default();

        let mut calls = Vec::new();
        {
            let mut pair_metadata_cache = self.pair_cache.lock().unwrap();

            for pair_input in pair_inputs {
                let pair_address = pair_input.address();
                if async_values.contains_key(pair_address)
                    || pair_results.contains_key(pair_address)
                {
                    continue;
                }

                if let Some(pair_metadata_async_value) = pair_metadata_cache.get(pair_address) {
                    async_values.insert(
                        *pair_address,
                        pair_metadata_async_value.get_receiver_or_value(),
                    );
                } else {
                    calls.extend(pair_input.calls());
                    pair_metadata_cache.put(*pair_address, AsyncValue::new());
                    pair_results.insert(*pair_address, (pair_input, Vec::new()));
                }
            }
        }

        let call_targets = calls
            .iter()
            .map(|call| call.target.clone())
            .collect::<Vec<_>>();

        let (multicall_results, resolved_pair_metadatas) = tokio::try_join!(
            multicall(Arc::clone(&self.inner), calls, block_id),
            resolve_pair_metadata_async_values(async_values)
        )?;

        multicall_results
            .into_iter()
            .zip(call_targets.into_iter())
            .for_each(|(result, call_target)| {
                let (_, results) = pair_results.get_mut(&call_target).unwrap();
                results.push(result);
            });

        let mut fetched_pair_metadatas = {
            let mut pair_metadata_cache = self.pair_cache.lock().unwrap();

            pair_results.into_iter().fold(
                FnvHashMap::default(),
                |mut acc, (pair_address, (pair_input, results))| {
                    let pair = pair_input.decode(results).ok().into();

                    if let Some(async_value) = pair_metadata_cache.get_mut(&pair_address) {
                        async_value.set(pair);
                    }

                    if let Some(pair) = pair {
                        acc.insert(pair_address, pair);
                    }

                    acc
                },
            )
        };

        fetched_pair_metadatas.extend(resolved_pair_metadatas);

        Ok(fetched_pair_metadatas)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config,
        primitives::{IndexedTrade, Pair, PairInput, UniswapV2IndexedTrade, UniswapV2Pair},
        providers::rpc_provider::new_http_signer_provider,
    };

    use alloy::primitives::{address, Address, U256};

    use eyre::Result;

    #[tokio::test]
    pub async fn test_get_pair_metadatas() -> Result<()> {
        let rpc_provider = new_http_signer_provider(&config::RPC_URL, None).await?;

        let indexed_trades = vec![
            IndexedTrade::UniswapV2(UniswapV2IndexedTrade::new(
                address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA"),
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                Address::ZERO,
            )),
            IndexedTrade::UniswapV2(UniswapV2IndexedTrade::new(
                address!("3c6554c1EF9845d629d333A24Ef1b13fCbC89577"),
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                U256::ZERO,
                Address::ZERO,
            )),
        ];

        let res = rpc_provider
            .indexed_trade_provider()
            .get_pairs(
                indexed_trades.iter().map(|t| PairInput::from(t)).collect(),
                None,
            )
            .await?;

        assert_eq!(res.len(), 2);
        assert_eq!(
            res.get(&address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA")),
            Some(&Pair::UniswapV2(UniswapV2Pair::new(
                address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA"),
                address!("4200000000000000000000000000000000000006"),
                address!("9a26F5433671751C3276a065f57e5a02D2817973")
            )))
        );
        assert_eq!(
            res.get(&address!("3c6554c1EF9845d629d333A24Ef1b13fCbC89577")),
            Some(&Pair::UniswapV2(UniswapV2Pair::new(
                address!("3c6554c1EF9845d629d333A24Ef1b13fCbC89577"),
                address!("4200000000000000000000000000000000000006"),
                address!("5e9fE073Df7Ce50E91EB9CBb010B99EF6035a97D")
            )))
        );

        Ok(())
    }
}
