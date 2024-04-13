use super::CommittedTrade;

use crate::{
    abi::{erc20, uniswap_v2_router},
    config,
    rpc_provider::RpcProvider,
};

use alloy::{
    primitives::{uint, Address, BlockNumber, U256},
    rpc::types::{eth::TransactionRequest, trace::parity::TraceType},
};

use eyre::Result;

const BP_FACTOR: U256 = uint!(10000_U256);
const MAX_TRADE_SIZE_PRICE_IMPACT_BP: U256 = uint!(50_U256);
const MAX_TRADE_SIZE_WEI: U256 = uint!(1000000000000000000_U256);
const DEADLINE_BUFFER: u64 = 30;

pub struct OpenPositionRequest {
    pair_address: Address,
    token_address: Address,
    block_number: BlockNumber,
    block_timestamp: u64,
    weth_reserve: U256,
    token_reserve: U256,
}

impl OpenPositionRequest {
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
    pub fn pair_address(&self) -> &Address {
        &self.pair_address
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

    pub fn into_sealed(self, signer_address: Address) -> SealedOpenPositionRequest {
        let (eth_amount_in, output_token_amount_min) = self.swap_params();
        let tx_request = uniswap_v2_router::swap_exact_eth_for_tokens_tx_request(
            signer_address,
            eth_amount_in,
            output_token_amount_min,
            self.token_address,
            U256::from(self.block_timestamp + 30),
        );

        SealedOpenPositionRequest::new(self, signer_address, tx_request)
    }
}

pub struct SealedOpenPositionRequest {
    open_position_request: OpenPositionRequest,
    signer_address: Address,
    tx_request: TransactionRequest,
}

impl SealedOpenPositionRequest {
    pub fn new(
        open_position_request: OpenPositionRequest,
        signer_address: Address,
        tx_request: TransactionRequest,
    ) -> Self {
        Self {
            open_position_request,
            signer_address,
            tx_request,
        }
    }

    #[inline]
    pub fn tx_request(&self) -> &TransactionRequest {
        &self.tx_request
    }

    #[inline]
    pub fn open_position_request(&self) -> &OpenPositionRequest {
        &self.open_position_request
    }

    pub async fn send(&self, rpc_provider: &RpcProvider) -> Result<CommittedTrade> {
        rpc_provider
            .send_transaction(self.tx_request.clone())
            .await
            .and_then(|receipt| (&receipt).try_into())
    }

    pub async fn trace(&self, rpc_provider: &RpcProvider) -> Result<()> {
        let trace_type_trace = vec![TraceType::Trace];

        // Calculate the expected close swap params
        let (close_input_token_amount, close_output_eth_amount_min) = {
            let (open_eth_amount_in, output_token_amount_min) =
                self.open_position_request.swap_params();
            let output_eth_amount_min = uniswap_v2_router::get_amount_out(
                output_token_amount_min,
                self.open_position_request.token_reserve - output_token_amount_min,
                self.open_position_request.weth_reserve + open_eth_amount_in,
            );

            (output_token_amount_min, output_eth_amount_min)
        };

        let trace_calls = vec![
            // swap exact eth for tokens
            (self.tx_request.clone(), trace_type_trace.clone()),
            // get signer token balance
            (
                erc20::balance_of_tx_request(
                    self.signer_address,
                    self.open_position_request.token_address,
                ),
                trace_type_trace.clone(),
            ),
            // approve token balance for router
            (
                erc20::approve_tx_request(
                    self.signer_address,
                    self.open_position_request.token_address,
                    *config::UNISWAP_V2_ROUTER_02_ADDRESS,
                    U256::MAX,
                ),
                trace_type_trace.clone(),
            ),
            // swap exact tokens for eth
            (
                uniswap_v2_router::swap_exact_tokens_for_eth_tx_request(
                    self.signer_address,
                    close_input_token_amount,
                    close_output_eth_amount_min,
                    self.open_position_request.token_address,
                    self.open_position_request.deadline(),
                ),
                trace_type_trace.clone(),
            ),
        ];

        let trace_results = rpc_provider
            .trace_call_many(
                &trace_calls,
                Some(self.open_position_request.block_number.into()),
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{config, rpc_provider::RpcProvider};

    use super::OpenPositionRequest;

    use alloy::primitives::address;
    use eyre::Result;

    // FIXME: Needs an RPC that supports trace_callMany
    #[ignore]
    #[tokio::test]
    async fn test_trace() -> Result<()> {
        let rpc_provider = RpcProvider::new(&config::RPC_URL).await?;
        let token_address = address!("5e9fe073df7ce50e91eb9cbb010b99ef6035a97d");
        let pair_address = address!("3c6554c1ef9845d629d333a24ef1b13fcbc89577");
        let block_number = 13119629;

        let (block_header_result, pair_reserves_result) = tokio::join!(
            rpc_provider.get_block_header(block_number),
            rpc_provider.get_uniswap_v2_pair_reserves(pair_address, Some(block_number.into()))
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

        let open_position_request = OpenPositionRequest::new(
            pair_address,
            token_address,
            weth_reserve,
            token_reserve,
            block_number,
            block_timestamp,
        );

        open_position_request
            .into_sealed(rpc_provider.signer_address())
            .trace(&rpc_provider)
            .await
    }
}
