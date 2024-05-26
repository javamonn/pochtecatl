use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Log, TxKind},
    rpc::types::eth::{Log as RpcLog, TransactionRequest},
    sol,
    sol_types::{SolCall, SolEvent, SolValue},
};

sol! {
    interface IUniswapV2Pair {
        event Swap(
            address indexed sender,
            uint amount0In,
            uint amount1In,
            uint amount0Out,
            uint amount1Out,
            address indexed to
        );
        event Sync(uint112 reserve0, uint112 reserve1);

        function token0() external view returns (address);
        function token1() external view returns (address);
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
    }
}

pub fn try_parse_swap_event(log: &RpcLog) -> Option<Log<IUniswapV2Pair::Swap>> {
    match log.topics().get(0) {
        Some(event_signature) if *event_signature == IUniswapV2Pair::Swap::SIGNATURE_HASH => {
            IUniswapV2Pair::Swap::decode_log(&log.inner, cfg!(debug_assertions)).ok()
        }
        _ => None,
    }
}

pub fn try_parse_sync_event(log: &RpcLog) -> Option<Log<IUniswapV2Pair::Sync>> {
    match log.topics().get(0) {
        Some(event_signature) if *event_signature == IUniswapV2Pair::Sync::SIGNATURE_HASH => {
            IUniswapV2Pair::Sync::decode_log(&log.inner, cfg!(debug_assertions)).ok()
        }
        _ => None,
    }
}

pub fn get_reserves_tx_request(
    signer_address: Address,
    pair_address: Address,
) -> TransactionRequest {
    let data = IUniswapV2Pair::getReservesCall {}.abi_encode();

    TransactionRequest::default()
        .with_from(signer_address)
        .with_to(Into::<TxKind>::into(pair_address))
        .with_input(data.abi_encode().into())
}
