use super::TradeRequest;

use crate::rpc_provider::RpcProvider;

use alloy::{primitives::Address, rpc::types::eth::TransactionRequest};

pub struct UniswapV2CloseTradeRequest {}

impl UniswapV2CloseTradeRequest {
    pub fn new() -> Self {
        Self {}
    }
}

impl TradeRequest for UniswapV2CloseTradeRequest {
    async fn trace(&self, _rpc_provider: &RpcProvider) -> eyre::Result<()> {
        unimplemented!()
    }

    fn address(&self) -> &Address {
        unimplemented!()
    }

    fn as_transaction_request(&self, _signer_address: Address) -> TransactionRequest {
        unimplemented!()
    }
}
