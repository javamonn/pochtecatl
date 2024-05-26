use pochtecatl_primitives::{IndexedTrade, Pair, RpcProvider, TradeMetadata, TradeRequestOp};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockNumber, TxHash, U256},
    providers::Provider,
    rpc::types::eth::{Block, BlockTransactions, TransactionRequest},
    transports::Transport,
};

use eyre::{eyre, OptionExt, Result};

pub trait TradeControllerRequest {
    fn token_address(&self) -> &Address;
    fn op(&self) -> &TradeRequestOp;

    async fn trace<T, P>(&self, rpc_provider: &RpcProvider<T, P>) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;

    fn simulate_trade_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> impl std::future::Future<Output = Result<TradeMetadata>> + Send
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;

    fn make_trade_transaction_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> impl std::future::Future<Output = Result<TransactionRequest>> + Send
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;
}

#[derive(Debug)]
pub struct TradeRequest {
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub op: TradeRequestOp,
    pub pair: Pair,
}

impl TradeRequest {
    pub fn open(block_number: BlockNumber, block_timestamp: u64, pair: Pair) -> Self {
        Self {
            block_number,
            block_timestamp,
            pair,
            op: TradeRequestOp::Open,
        }
    }

    pub fn close(
        block_number: BlockNumber,
        block_timestamp: u64,
        pair: Pair,
        open_trade: IndexedTrade,
        open_trade_tx_hash: TxHash,
    ) -> Self {
        Self {
            block_number,
            block_timestamp,
            pair,
            op: TradeRequestOp::Close {
                open_trade,
                open_trade_tx_hash,
            },
        }
    }
}

impl TradeControllerRequest for TradeRequest {
    fn token_address(&self) -> &Address {
        self.pair.token_address()
    }

    fn op(&self) -> &TradeRequestOp {
        &self.op
    }

    async fn trace<T, P>(&self, rpc_provider: &RpcProvider<T, P>) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        self.pair
            .trace_trade_request(&self.op, self.block_number, rpc_provider)
            .await
    }

    async fn simulate_trade_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TradeMetadata>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        if !*crate::config::IS_BACKTEST {
            return Err(eyre!("Only avaialble in backtest"));
        }

        let indexed_trade = self
            .pair
            .simulate_trade_request(&self.op, self.block_number, rpc_provider)
            .await?;

        // Gets the effective_gas_price of the tx in the middle of the block we would've
        // confirmed in while backtesting, and multiples it by an estimate of the gas used
        // by the trade to get the estimated gas fee.
        let estimated_gas_fee = {
            let median_tx_hash = rpc_provider
                .block_provider()
                .get_block(self.block_number)
                .await
                .ok()
                .and_then(|block| match block {
                    Some(Block {
                        transactions: BlockTransactions::Hashes(hashes),
                        ..
                    }) => hashes.get(hashes.len() / 2).cloned(),
                    _ => None,
                })
                .ok_or_eyre("Failed to get median tx hash")?;

            let median_tx_effictive_gas_price = rpc_provider
                .get_transaction_receipt(median_tx_hash)
                .await
                .ok()
                .and_then(|receipt| receipt.map(|receipt| receipt.effective_gas_price))
                .ok_or_eyre("Failed to get median tx receipt")?;

            U256::from(median_tx_effictive_gas_price) * self.pair.estimate_trade_gas()
        };

        Ok(TradeMetadata::new(
            TxHash::random(),
            self.block_number,
            self.block_timestamp,
            self.op.clone(),
            *self.pair.token_address(),
            estimated_gas_fee,
            indexed_trade,
        ))
    }

    async fn make_trade_transaction_request<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> Result<TransactionRequest>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static,
    {
        self.pair
            .make_trade_transaction_request(
                &self.op,
                self.block_number,
                self.block_timestamp,
                rpc_provider,
            )
            .await
    }
}
