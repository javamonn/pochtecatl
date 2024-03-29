use super::log_parser::Block;
use crate::rpc_provider::RpcProvider;
use tokio::task::JoinSet;

use alloy::primitives::Address;

use fnv::FnvHashMap;
use std::sync::Arc;

pub async fn get_block_uniswap_v2_pair_token_addresses(
    rpc_provider: Arc<RpcProvider>,
    block: &Block,
) -> FnvHashMap<Address, (Address, Address)> {
    let mut tasks = JoinSet::new();
    let mut output =
        FnvHashMap::with_capacity_and_hasher(block.uniswap_v2_trades.len(), Default::default());

    for pair_address in block.uniswap_v2_trades.keys().copied() {
        let rpc_provider = Arc::clone(&rpc_provider);
        tasks.spawn(async move {
            match rpc_provider
                .get_uniswap_v2_pair_token_addresses(pair_address)
                .await
            {
                Ok(token_addresses) => Some((pair_address, token_addresses)),
                Err(err) => {
                    log::error!(
                        "get_uniswap_v2_pair_token_addresses {} failed: {:?}",
                        pair_address,
                        err
                    );
                    None
                }
            }
        });
    }

    while let Some(pair_token_addresses) = tasks.join_next().await {
        match pair_token_addresses {
            Ok(Some((pair_address, token_addresses))) => {
                output.insert(pair_address, token_addresses);
            }
            Ok(None) => {}
            Err(err) => {
                log::error!("join_next error: {:?}", err);
            }
        }
    }

    output
}
