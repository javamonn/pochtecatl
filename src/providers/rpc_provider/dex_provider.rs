use super::{multicall::multicall, AsyncReceiverOrValue, AsyncValue};
use crate::primitives::{Pair, PairInput};

use alloy::{
    network::Ethereum, primitives::Address, providers::Provider, rpc::types::eth::BlockId,
    transports::Transport,
};

use eyre::Result;
use fnv::FnvHashMap;
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use tracing::debug;

pub struct DexProvider<T: Transport + Clone, P: Provider<T, Ethereum>> {
    pair_cache: Mutex<LruCache<Address, AsyncValue<Option<Pair>>>>,
    inner: Arc<P>,
    _transport_marker: std::marker::PhantomData<T>,
}

impl<T, P> DexProvider<T, P>
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
            AsyncReceiverOrValue::resolve_map(async_values)
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
                    let pair = pair_input
                        .decode(results)
                        .inspect_err(|err| {
                            debug!(
                                "Failed to decode pair metadata for pair {:?}: {}",
                                pair_input,
                                err
                            );
                        })
                        .ok()
                        .into();

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
        primitives::{Pair, UniswapV2Pair, UniswapV2PairInput, UniswapV3Pair, UniswapV3PairInput},
        providers::rpc_provider::new_http_signer_provider,
    };

    use alloy::primitives::address;
    use eyre::Result;

    #[tokio::test]
    pub async fn test_get_pairs() -> Result<()> {
        let rpc_provider = new_http_signer_provider(&config::RPC_URL, None).await?;

        let res = rpc_provider
            .dex_provider()
            .get_pairs(
                vec![
                    UniswapV2PairInput::new(address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA"))
                        .into(),
                    UniswapV2PairInput::new(address!("3c6554c1EF9845d629d333A24Ef1b13fCbC89577"))
                        .into(),
                    UniswapV3PairInput::new(address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA"))
                        .into(),
                ],
                None,
            )
            .await?;

        assert_eq!(res.len(), 3);
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

        assert_eq!(
            res.get(&address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA")),
            Some(&Pair::UniswapV3(UniswapV3Pair::new(
                address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA"),
                address!("4200000000000000000000000000000000000006"),
                address!("4ed4E862860beD51a9570b96d89aF5E1B0Efefed"),
                3000
            )))
        );

        Ok(())
    }
}
