use alloy::{
    sol,
    primitives::Log,
    rpc::types::eth::Log as RpcLog,
    sol_types::SolEvent,
};

sol! {
    interface IUniswapV3Pool {
        event Swap(
            address indexed sender,
            address indexed recipient,
            int256 amount0,
            int256 amount1,
            uint160 sqrtPriceX96,
            uint128 liquidity,
            int24 tick
        );

        function token0() external view returns (address);
        function token1() external view returns (address);
    }
}

pub fn try_parse_swap_event(log: &RpcLog) -> Option<Log<IUniswapV3Pool::Swap>> {
    match log.topics().get(0) {
        Some(event_signature) if *event_signature == IUniswapV3Pool::Swap::SIGNATURE_HASH => {
            IUniswapV3Pool::Swap::decode_log(&log.inner, cfg!(debug_assertions)).ok()
        }
        _ => None,
    }
}
