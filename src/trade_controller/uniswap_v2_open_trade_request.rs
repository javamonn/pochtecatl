use super::{backtest_util::estimate_gas_fee, TradeMetadata, TradeRequest};

use crate::{abi::uniswap_v2_router, config, indexer::UniswapV2PairTrade, providers::RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::{uint, Address, BlockNumber, U256},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    transports::Transport,
};

use eyre::Result;

const BP_FACTOR: U256 = uint!(10000_U256);
const MAX_TRADE_SIZE_PRICE_IMPACT_BP: U256 = uint!(50_U256);
const MAX_TRADE_SIZE_WEI: U256 = uint!(1000000000000000000_U256);
const DEADLINE_BUFFER: u64 = 30;

pub struct UniswapV2OpenTradeRequest {
    pair_address: Address,
    token_address: Address,
    block_number: BlockNumber,
    block_timestamp: u64,
    weth_reserve: U256,
    token_reserve: U256,
}

impl UniswapV2OpenTradeRequest {
    pub fn new(
        pair_address: Address,
        token_address: Address,
        weth_reserve: U256,
        token_reserve: U256,
        block_number: BlockNumber,
        block_timestamp: u64,
    ) -> Self {
        Self {
            block_number,
            block_timestamp,
            pair_address,
            token_address,
            weth_reserve,
            token_reserve,
        }
    }

    #[inline]
    pub fn deadline(&self) -> U256 {
        U256::from(self.block_timestamp + DEADLINE_BUFFER)
    }

    pub fn swap_params(&self) -> (U256, U256) {
        let eth_amount_in = {
            let max_for_price_impact =
                (MAX_TRADE_SIZE_PRICE_IMPACT_BP * self.weth_reserve) / BP_FACTOR;
            if max_for_price_impact < MAX_TRADE_SIZE_WEI {
                max_for_price_impact
            } else {
                MAX_TRADE_SIZE_WEI
            }
        };
        let output_token_amount_min =
            uniswap_v2_router::get_amount_out(eth_amount_in, self.weth_reserve, self.token_reserve);

        (eth_amount_in, output_token_amount_min)
    }
}

impl<T, P> TradeRequest<UniswapV2PairTrade, T, P> for UniswapV2OpenTradeRequest
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    fn address(&self) -> &Address {
        &self.pair_address
    }

    fn as_transaction_request(&self, signer_address: Address) -> TransactionRequest {
        let (eth_amount_in, output_token_amount_min) = self.swap_params();
        uniswap_v2_router::swap_exact_eth_for_tokens_tx_request(
            signer_address,
            eth_amount_in,
            output_token_amount_min,
            self.token_address,
            U256::from(self.block_timestamp + 30),
        )
    }

    async fn as_backtest_trade_metadata(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TradeMetadata<UniswapV2PairTrade>> {
        let (eth_amount_in, output_token_amount_min) = self.swap_params();
        let trade = if self.token_address < *config::WETH_ADDRESS {
            // token0 is token, token1 is weth
            UniswapV2PairTrade::new(
                U256::ZERO,
                eth_amount_in,
                output_token_amount_min,
                U256::ZERO,
                self.token_reserve - output_token_amount_min,
                self.weth_reserve + eth_amount_in,
                rpc_provider.signer_address().clone(),
            )
        } else {
            // token0 is weth, token1 is token
            UniswapV2PairTrade::new(
                eth_amount_in,
                U256::ZERO,
                U256::ZERO,
                output_token_amount_min,
                self.weth_reserve + eth_amount_in,
                self.token_reserve - output_token_amount_min,
                rpc_provider.signer_address().clone(),
            )
        };

        let gas_fee =
            estimate_gas_fee(&rpc_provider, uint!(130000_U256), self.block_number + 1).await?;

        Ok(TradeMetadata::new(
            self.block_number + 1,
            self.block_timestamp + *config::AVERAGE_BLOCK_TIME_SECONDS,
            gas_fee,
            trade,
        ))
    }

    async fn trace(&self, _rpc_provider: &RpcProvider<T, P>) -> Result<()> {
        Ok(())

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
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config, providers::rpc_provider::new_http_signer_provider, trade_controller::TradeRequest,
    };

    use super::UniswapV2OpenTradeRequest;

    use alloy::primitives::address;
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

        let trade_request = UniswapV2OpenTradeRequest::new(
            pair_address,
            token_address,
            weth_reserve,
            token_reserve,
            block_number,
            block_timestamp,
        );

        trade_request.trace(&rpc_provider).await
    }
}
