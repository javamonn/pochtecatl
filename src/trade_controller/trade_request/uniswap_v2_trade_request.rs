use super::{super::ParsedTrade, TradeRequestIntent};
use crate::{
    abi::uniswap_v2_router, config, primitives::UniswapV2PairTrade, providers::RpcProvider,
};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockNumber, U256},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    transports::Transport,
};

use eyre::Result;

#[derive(Debug)]
pub struct UniswapV2TradeRequest {
    pair_address: Address,
    token_address: Address,
    block_number: BlockNumber,
    block_timestamp: u64,
    weth_reserve: U256,
    token_reserve: U256,
    intent: TradeRequestIntent,
}

impl UniswapV2TradeRequest {
    const DEADLINE_BUFFER: u64 = 30;

    pub fn new(
        pair_address: Address,
        token_address: Address,
        block_number: BlockNumber,
        block_timestamp: u64,
        weth_reserve: U256,
        token_reserve: U256,
        intent: TradeRequestIntent,
    ) -> Self {
        Self {
            pair_address,
            token_address,
            block_number,
            block_timestamp,
            weth_reserve,
            token_reserve,
            intent,
        }
    }

    pub fn intent(&self) -> &TradeRequestIntent {
        &self.intent
    }

    pub fn pair_address(&self) -> &Address {
        &self.pair_address
    }

    pub fn block_number(&self) -> &BlockNumber {
        &self.block_number
    }

    pub fn block_timestamp(&self) -> &u64 {
        &self.block_timestamp
    }

    pub async fn trace<T, P>(&self, _rpc_provider: &RpcProvider<T, P>) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        match self.intent {
            TradeRequestIntent::Close { .. } => Ok(()),
            TradeRequestIntent::Open { eth_amount_in } => {
                // TODO: fix tracing impl
                /*
                let trace_type_trace = vec![TraceType::Trace];

                // Calculate the expected close swap params
                let (close_input_token_amount, close_output_eth_amount_min) = {
                    let (open_eth_amount_in, output_token_amount_min) = self.swap_params();
                    let output_eth_amount_min = uniswap_v2_router::get_amount_out(
                        output_token_amount_min,
                        self.token_reserve - output_token_amount_min,
                        self.weth_reserve + open_eth_amount_in,
                    );

                    (output_token_amount_min, output_eth_amount_min)
                };

                let trace_calls = vec![
                    // swap exact eth for tokens
                    (
                        self.as_transaction_request(rpc_provider.signer_address().clone()),
                        trace_type_trace.clone(),
                    ),
                    // get signer token balance
                    (
                        erc20::balance_of_tx_request(
                            rpc_provider.signer_address().clone(),
                            self.token_address,
                        ),
                        trace_type_trace.clone(),
                    ),
                    // approve token balance for router
                    (
                        erc20::approve_tx_request(
                            rpc_provider.signer_address().clone(),
                            self.token_address,
                            *config::UNISWAP_V2_ROUTER_02_ADDRESS,
                            U256::MAX,
                        ),
                        trace_type_trace.clone(),
                    ),
                    // swap exact tokens for eth
                    (
                        uniswap_v2_router::swap_exact_tokens_for_eth_tx_request(
                            rpc_provider.signer_address().clone(),
                            close_input_token_amount,
                            close_output_eth_amount_min,
                            self.token_address,
                            self.deadline(),
                        ),
                        trace_type_trace.clone(),
                    ),
                ];

                let trace_results = rpc_provider
                    .trace_call_many(
                        &trace_calls,
                        Some(self.block_number.into()),
                    )
                    .await?;

                if trace_results
                    .trace
                    .iter()
                    .any(|trace| trace.error.is_some())
                {
                    return Err(eyre::eyre!(
                        "uniswap_v2_position_is_valid: trace error: {:?}",
                        trace_results
                    ));
                }
                */
                Ok(())
            }
        }
    }

    pub fn as_transaction_request(&self, signer_address: Address) -> TransactionRequest {
        match self.intent {
            TradeRequestIntent::Open { eth_amount_in } => {
                let min_token_amount_out = uniswap_v2_router::get_amount_out(
                    eth_amount_in,
                    self.weth_reserve,
                    self.token_reserve,
                );

                uniswap_v2_router::swap_exact_eth_for_tokens_tx_request(
                    signer_address,
                    min_token_amount_out,
                    eth_amount_in,
                    self.token_address,
                    U256::from(self.block_timestamp + Self::DEADLINE_BUFFER),
                )
            }
            TradeRequestIntent::Close { token_amount_in } => {
                let min_eth_amount_out = uniswap_v2_router::get_amount_out(
                    token_amount_in,
                    self.token_reserve,
                    self.weth_reserve,
                );

                uniswap_v2_router::swap_exact_tokens_for_eth_tx_request(
                    signer_address,
                    token_amount_in,
                    min_eth_amount_out,
                    self.token_address,
                    U256::from(self.block_timestamp + Self::DEADLINE_BUFFER),
                )
            }
        }
    }

    pub fn estimate_parsed_trade(&self, signer_address: Address) -> ParsedTrade {
        match self.intent {
            TradeRequestIntent::Open { eth_amount_in } => {
                let output_token_amount_min = uniswap_v2_router::get_amount_out(
                    eth_amount_in,
                    self.weth_reserve,
                    self.token_reserve,
                );
                if self.token_address < *config::WETH_ADDRESS {
                    // token0 is token, token1 is weth
                    ParsedTrade::UniswapV2PairTrade(UniswapV2PairTrade::new(
                        U256::ZERO,
                        eth_amount_in,
                        output_token_amount_min,
                        U256::ZERO,
                        self.token_reserve - output_token_amount_min,
                        self.weth_reserve + eth_amount_in,
                        signer_address.clone(),
                    ))
                } else {
                    // token0 is weth, token1 is token
                    ParsedTrade::UniswapV2PairTrade(UniswapV2PairTrade::new(
                        eth_amount_in,
                        U256::ZERO,
                        U256::ZERO,
                        output_token_amount_min,
                        self.weth_reserve + eth_amount_in,
                        self.token_reserve - output_token_amount_min,
                        signer_address.clone(),
                    ))
                }
            }
            TradeRequestIntent::Close { token_amount_in } => {
                let min_eth_amount_out = uniswap_v2_router::get_amount_out(
                    token_amount_in,
                    self.token_reserve,
                    self.weth_reserve,
                );

                if self.token_address < *config::WETH_ADDRESS {
                    // token0 is token, token1 is weth
                    ParsedTrade::UniswapV2PairTrade(UniswapV2PairTrade::new(
                        token_amount_in,
                        U256::ZERO,
                        U256::ZERO,
                        min_eth_amount_out,
                        self.token_reserve + token_amount_in,
                        self.weth_reserve - min_eth_amount_out,
                        signer_address.clone(),
                    ))
                } else {
                    // token1 is token, token0 is weth
                    ParsedTrade::UniswapV2PairTrade(UniswapV2PairTrade::new(
                        U256::ZERO,
                        token_amount_in,
                        min_eth_amount_out,
                        U256::ZERO,
                        self.weth_reserve - min_eth_amount_out,
                        self.token_reserve + token_amount_in,
                        signer_address.clone(),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config, providers::rpc_provider::new_http_signer_provider,
        trade_controller::TradeRequestIntent,
    };
    use super::UniswapV2TradeRequest;

    use alloy::primitives::{address, U256};

    use eyre::Result;

    // FIXME: Needs an RPC that supports trace_callMany
    #[ignore]
    #[tokio::test]
    async fn test_trace() -> Result<()> {
        let rpc_provider = new_http_signer_provider(&config::RPC_URL, None).await?;
        let token_address = address!("5e9fe073df7ce50e91eb9cbb010b99ef6035a97d");
        let pair_address = address!("3c6554c1ef9845d629d333a24ef1b13fcbc89577");
        let block_number = 13119629;

        let (block_header_result, pair_reserves_result) = tokio::join!(
            rpc_provider.block_provider().get_block_header(block_number),
            rpc_provider
                .uniswap_v2_pair_provider()
                .get_uniswap_v2_pair_reserves(pair_address, Some(block_number.into()))
        );

        let block_timestamp = block_header_result?
            .expect("block header not found")
            .timestamp
            .to::<u64>();
        let (token_reserve, weth_reserve) = {
            let (reserve0, reserve1) = pair_reserves_result?;
            if token_address < *config::WETH_ADDRESS {
                (reserve0, reserve1)
            } else {
                (reserve1, reserve0)
            }
        };

        let trade_request = UniswapV2TradeRequest::new(
            pair_address,
            token_address,
            block_number,
            block_timestamp,
            weth_reserve,
            token_reserve,
            TradeRequestIntent::Open {
                eth_amount_in: U256::ZERO,
            },
        );

        trade_request.trace(&rpc_provider).await
    }
}
