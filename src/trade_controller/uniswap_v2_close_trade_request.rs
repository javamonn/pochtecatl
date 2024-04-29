use super::{backtest_util::estimate_gas_fee, TradeMetadata, TradeRequest};

use crate::{
    abi::uniswap_v2_router, config, indexer::UniswapV2PairTrade, rpc_provider::RpcProvider,
};

use alloy::{
    primitives::{uint, Address, BlockNumber, U256},
    rpc::types::eth::TransactionRequest,
};
use eyre::Result;

const DEADLINE_BUFFER: u64 = 30;

pub struct UniswapV2CloseTradeRequest {
    pair_address: Address,
    token_address: Address,
    block_number: BlockNumber,
    block_timestamp: u64,
    weth_reserve: U256,
    token_reserve: U256,
    token_amount_in: U256,
    min_eth_amount_out: U256,
}

impl UniswapV2CloseTradeRequest {
    #[inline]
    pub fn deadline(&self) -> U256 {
        U256::from(self.block_timestamp + DEADLINE_BUFFER)
    }
}

impl UniswapV2CloseTradeRequest {
    pub fn new(
        pair_address: Address,
        token_address: Address,
        weth_reserve: U256,
        token_reserve: U256,
        token_amount_in: U256,
        block_number: BlockNumber,
        block_timestamp: u64,
    ) -> Self {
        let min_eth_amount_out =
            uniswap_v2_router::get_amount_out(token_amount_in, token_reserve, weth_reserve);

        Self {
            block_number,
            block_timestamp,
            pair_address,
            token_address,
            token_amount_in,
            token_reserve,
            weth_reserve,
            min_eth_amount_out,
        }
    }
}

impl TradeRequest<UniswapV2PairTrade> for UniswapV2CloseTradeRequest {
    async fn trace(&self, _rpc_provider: &RpcProvider) -> Result<()> {
        Ok(())

        // TODO: fix tracing impl
    }

    fn address(&self) -> &Address {
        &self.pair_address
    }

    fn as_transaction_request(&self, signer_address: Address) -> TransactionRequest {
        uniswap_v2_router::swap_exact_tokens_for_eth_tx_request(
            signer_address,
            self.token_amount_in,
            self.min_eth_amount_out,
            self.token_address,
            self.deadline(),
        )
    }

    async fn as_backtest_trade_metadata(
        &self,
        rpc_provider: &RpcProvider,
    ) -> Result<TradeMetadata<UniswapV2PairTrade>> {
        let trade = if self.token_address < *config::WETH_ADDRESS {
            // token0 is token, token1 is weth
            UniswapV2PairTrade::new(
                self.token_amount_in,
                U256::ZERO,
                U256::ZERO,
                self.min_eth_amount_out,
                self.token_reserve + self.token_amount_in,
                self.weth_reserve - self.min_eth_amount_out,
                rpc_provider.signer_address().clone(),
            )
        } else {
            // token1 is token, token0 is weth
            UniswapV2PairTrade::new(
                U256::ZERO,
                self.token_amount_in,
                self.min_eth_amount_out,
                U256::ZERO,
                self.weth_reserve - self.min_eth_amount_out,
                self.token_reserve + self.token_amount_in,
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
}
