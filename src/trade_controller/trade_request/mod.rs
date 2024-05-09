pub use trade_request_intent::TradeRequestIntent;
pub use uniswap_v2_trade_request::UniswapV2TradeRequest;

mod trade_request_intent;
mod uniswap_v2_trade_request;

use super::{backtest_util::estimate_gas_fee, TradeControllerRequest, TradeMetadata};
use crate::{config, providers::RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::{uint, Address, BlockNumber},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    transports::Transport,
};

use eyre::{eyre, Result};

#[derive(Debug)]
pub enum TradeRequest {
    UniswapV2(UniswapV2TradeRequest),
}

impl TradeControllerRequest for TradeRequest {
    fn pair_address(&self) -> &Address {
        match self {
            TradeRequest::UniswapV2(r) => r.pair_address(),
        }
    }

    fn as_transaction_request(&self, signer_address: Address) -> TransactionRequest {
        match self {
            TradeRequest::UniswapV2(r) => r.as_transaction_request(signer_address),
        }
    }

    async fn trace<T, P>(&self, rpc_provider: &RpcProvider<T, P>) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        match self {
            TradeRequest::UniswapV2(r) => r.trace(rpc_provider).await,
        }
    }

    // Estimate the trade metadata that would result from ideal execution
    async fn estimate_trade_metadata<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TradeMetadata>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        if !*config::IS_BACKTEST {
            return Err(eyre!("Trade metadata is only available in backtest mode"));
        }

        let (gas_estimate, parsed_trade) = match self {
            TradeRequest::UniswapV2(t) => (
                uint!(130000_U256),
                t.estimate_parsed_trade(rpc_provider.signer_address().clone()),
            ),
        };

        let gas_fee =
            estimate_gas_fee(&rpc_provider, gas_estimate, self.block_number() + 1).await?;

        Ok(TradeMetadata::new(
            *self.block_number(),
            *self.block_timestamp(),
            gas_fee,
            parsed_trade,
        ))
    }

}

impl TradeRequest {
    pub fn intent(&self) -> &TradeRequestIntent {
        match self {
            TradeRequest::UniswapV2(r) => r.intent(),
        }
    }


    pub fn block_number(&self) -> &BlockNumber {
        match self {
            TradeRequest::UniswapV2(r) => r.block_number(),
        }
    }

    pub fn block_timestamp(&self) -> &u64 {
        match self {
            TradeRequest::UniswapV2(r) => r.block_timestamp(),
        }
    }

}
