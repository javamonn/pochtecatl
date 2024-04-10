use crate::abi::IUniswapV2Pair;

use alloy::{primitives::Log, rpc::types::eth::Log as RpcLog, sol_types::SolEvent};

pub fn parse(log: &RpcLog) -> Option<Log<IUniswapV2Pair::Sync>> {
    match log.topics().get(0) {
        Some(event_signature) if *event_signature == IUniswapV2Pair::Sync::SIGNATURE_HASH => {
            IUniswapV2Pair::Sync::decode_log_data(log.data(), cfg!(debug_assertions))
                .ok()
                .map(|data| Log {
                    address: log.address(),
                    data,
                })
        }
        _ => None,
    }
}
