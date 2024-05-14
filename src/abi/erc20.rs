use alloy::{
    network::TransactionBuilder,
    primitives::{Address, TxKind, U256},
    rpc::types::eth::TransactionRequest,
    sol,
    sol_types::SolCall,
};

sol! {
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
        function approve(address spender, uint256 value) external returns (bool);
    }
}

// TODO: used by disabled trace code
#[allow(dead_code)]
pub fn approve_tx_request(
    signer_address: Address,
    token_address: Address,
    spender: Address,
    value: U256,
) -> TransactionRequest {
    let data = IERC20::approveCall { spender, value };

    TransactionRequest::default()
        .with_from(signer_address)
        .with_to(Into::<TxKind>::into(token_address))
        .with_input(data.abi_encode().into())
}

// TODO: used by disabled trace code
#[allow(dead_code)]
pub fn balance_of_tx_request(
    signer_address: Address,
    token_address: Address,
) -> TransactionRequest {
    let data = IERC20::balanceOfCall {
        account: signer_address,
    };

    TransactionRequest::default()
        .with_from(signer_address)
        .with_to(Into::<TxKind>::into(token_address))
        .with_input(data.abi_encode().into())
}
