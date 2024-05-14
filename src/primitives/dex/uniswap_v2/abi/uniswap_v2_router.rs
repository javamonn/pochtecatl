use crate::config;

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, TxKind, U256},
    rpc::types::eth::TransactionRequest,
    sol,
    sol_types::SolCall,
};

sol! {
    interface IUniswapV2Router02 {
        function swapExactETHForTokens(
            uint amountOutMin,
            address[] calldata path,
            address to,
            uint deadline
        ) external payable returns (uint[] memory amounts);

        function swapExactTokensForETH(
            uint amountIn,
            uint amountOutMin,
            address[] calldata path,
            address to,
            uint deadline
        ) external returns (uint[] memory amounts);
   }
}

pub fn get_amount_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> U256 {
    let amount_in_with_fee = amount_in * U256::from(997);
    let numerator = amount_in_with_fee * reserve_out;
    let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

    numerator / denominator
}

pub fn swap_exact_eth_for_tokens_tx_request(
    signer_address: Address,
    input_eth_amount: U256,
    output_token_amount_min: U256,
    output_token_address: Address,
    deadline: U256,
) -> TransactionRequest {
    let data = IUniswapV2Router02::swapExactETHForTokensCall {
        amountOutMin: output_token_amount_min,
        path: vec![*config::WETH_ADDRESS, output_token_address],
        to: signer_address,
        deadline: U256::from(deadline),
    }
    .abi_encode();

    TransactionRequest::default()
        .with_from(signer_address)
        .with_to(Into::<TxKind>::into(*config::UNISWAP_V2_ROUTER_02_ADDRESS))
        .with_value(input_eth_amount)
        .with_input(data.into())
}

pub fn swap_exact_tokens_for_eth_tx_request(
    signer_address: Address,
    input_token_amount: U256,
    output_eth_amount_min: U256,
    input_token_address: Address,
    deadline: U256,
) -> TransactionRequest {
    let data = IUniswapV2Router02::swapExactTokensForETHCall {
        amountIn: input_token_amount,
        amountOutMin: output_eth_amount_min,
        path: vec![input_token_address, *config::WETH_ADDRESS],
        to: signer_address,
        deadline: U256::from(deadline),
    }
    .abi_encode();

    TransactionRequest::default()
        .with_from(signer_address)
        .with_to(Into::<TxKind>::into(*config::UNISWAP_V2_ROUTER_02_ADDRESS))
        .with_input(data.into())
}
