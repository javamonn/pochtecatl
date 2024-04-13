use crate::rpc_provider::RpcProvider;

use super::{ClosePositionRequest, OpenPositionRequest, TradePosition};

use alloy::primitives::Address;

use eyre::{eyre, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};

pub struct TradePositionController {
    rpc_provider: Arc<RpcProvider>,
    trade_positions: RwLock<FnvHashMap<Address, TradePosition>>,
}

impl TradePositionController {
    pub fn new(rpc_provider: Arc<RpcProvider>) -> Self {
        Self {
            rpc_provider,
            trade_positions: RwLock::new(FnvHashMap::default()),
        }
    }

    pub fn trade_positions(&self) -> &RwLock<FnvHashMap<Address, TradePosition>> {
        &self.trade_positions
    }

    pub async fn close_position(
        &self,
        mut ClosePositionRequest: ClosePositionRequest,
    ) -> Result<()> {
        unimplemented!()
    }

    fn remove_position(&self, pair_address: &Address) {
        let mut trade_positions = self.trade_positions.write().unwrap();
        trade_positions.remove(pair_address);
    }

    pub async fn open_position(&self, open_position_request: OpenPositionRequest) -> Result<()> {
        // ensure that we do not already have a position for this pair and add
        // the position to the store
        {
            let mut trade_positions = self.trade_positions.write().unwrap();
            if trade_positions.contains_key(open_position_request.pair_address()) {
                return Err(eyre!(
                    "Position already exists for pair {}",
                    open_position_request.pair_address()
                ));
            } else {
                trade_positions.insert(
                    *open_position_request.pair_address(),
                    TradePosition::PendingOpen,
                );
            }
        }

        let sealed = open_position_request.into_sealed(self.rpc_provider.signer_address());

        // Ensure the position is valid, otherwise remove the pending position from 
        // the store
        // FIXME: needs an rpc that supports tracing
        // sealed.trace(&self.rpc_provider).await.inspect_err(|_| {
        //    self.remove_position(sealed.open_position_request().pair_address());
        // })?;

        match sealed.send(&self.rpc_provider).await {
            Ok(committed_trade) => {
                // Add the committed position to the store
                {
                    let mut trade_positions = self.trade_positions.write().unwrap();
                    trade_positions.insert(
                        *sealed.open_position_request().pair_address(),
                        TradePosition::Open(committed_trade),
                    );
                }

                Ok(())
            }
            Err(err) => {
                // Remove the pending position from the store
                self.remove_position(sealed.open_position_request().pair_address());
                Err(err)
            }
        }
    }
}
