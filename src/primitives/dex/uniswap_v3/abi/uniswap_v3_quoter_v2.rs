use crate::config;

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, U256, TxKind},
    rpc::types::eth::TransactionRequest,
    sol_types::{SolValue, SolCall},
    sol
};

sol! {
    interface IQuoterV2 {
        struct QuoteExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint256 amountIn;
            uint24 fee;
            uint160 sqrtPriceLimitX96;
        }

        /// @notice Returns the amount out received for a given exact input but for a swap of a single pool
        /// @param params The params for the quote, encoded as `QuoteExactInputSingleParams`
        /// tokenIn The token being swapped in
        /// tokenOut The token being swapped out
        /// fee The fee of the token pool to consider for the pair
        /// amountIn The desired input amount
        /// sqrtPriceLimitX96 The price limit of the pool that cannot be exceeded by the swap
        /// @return amountOut The amount of `tokenOut` that would be received
        /// @return sqrtPriceX96After The sqrt price of the pool after the swap
        /// @return initializedTicksCrossed The number of initialized ticks that the swap crossed
        /// @return gasEstimate The estimate of the gas that the swap consumes
        function quoteExactInputSingle(QuoteExactInputSingleParams memory params)
            external
            returns (
                uint256 amountOut,
                uint160 sqrtPriceX96After,
                uint32 initializedTicksCrossed,
                uint256 gasEstimate
            );
        }
}

pub fn quote_exact_input_single_tx_request(
    signer_address: Address,
    token_in: Address,
    token_out: Address,
    amount_in: U256,
    fee: u32,
    sqrt_price_limit_x96: U256,
) -> TransactionRequest {
    let data = IQuoterV2::quoteExactInputSingleCall {
        params: IQuoterV2::QuoteExactInputSingleParams {
            tokenIn: token_in,
            tokenOut: token_out,
            amountIn: amount_in,
            fee,
            sqrtPriceLimitX96: sqrt_price_limit_x96,
        },
    }
    .abi_encode();

    TransactionRequest::default()
        .with_from(signer_address)
        .with_to(Into::<TxKind>::into(*config::UNISWAP_V3_QUOTER_V2_ADDRESS))
        .with_input(data.abi_encode().into())
}
