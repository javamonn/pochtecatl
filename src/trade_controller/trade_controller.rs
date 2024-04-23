use crate::{indexer::ParseableTrade, rpc_provider::RpcProvider};

use super::{Trade, TradeRequest, TransactionRequest};

use alloy::primitives::{Address, TxHash};

use eyre::{eyre, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};

pub struct TradeController<T: ParseableTrade> {
    rpc_provider: Arc<RpcProvider>,
    trade_positions: Arc<RwLock<FnvHashMap<Address, Trade<T>>>>,
}

impl<T: ParseableTrade + Send + Sync + 'static> TradeController<T> {
    pub fn new(rpc_provider: Arc<RpcProvider>) -> Self {
        Self {
            rpc_provider,
            trade_positions: Arc::new(RwLock::new(FnvHashMap::default())),
        }
    }

    pub fn trade_positions(&self) -> &RwLock<FnvHashMap<Address, Trade<T>>> {
        &self.trade_positions
    }

    pub async fn close_position<CloseTradeRequest: TradeRequest>(
        &self,
        _close_trade_request: CloseTradeRequest,
    ) -> Result<TxHash> {
        unimplemented!()
    }

    pub async fn open_position<OpenTradeRequest: TradeRequest>(
        &self,
        open_position_request: OpenTradeRequest,
    ) -> Result<()> {
        // ensure that we do not already have a position for this pair and add
        // the position to the store
        {
            let mut trade_positions = self.trade_positions.write().unwrap();
            if trade_positions.contains_key(open_position_request.address()) {
                return Err(eyre!(
                    "Position already exists for pair {}",
                    open_position_request.address()
                ));
            } else {
                trade_positions.insert(*open_position_request.address(), Trade::PendingOpen);
            }
        }

        // Ensure the position is valid, otherwise remove the pending position from
        // the store
        open_position_request
            .trace(&self.rpc_provider)
            .await
            .inspect_err(|_| {
                let mut trade_positions = self.trade_positions.write().unwrap();
                trade_positions.remove(open_position_request.address());
            })?;

        let address = open_position_request.address().clone();

        // Txs are committed in two steps:
        // 1. first as a pending tx to the network, which this method awaits the completion of
        // 2. second as a committed trade, which is awaited in a separate async block

        match TransactionRequest::from(
            open_position_request
                .as_transaction_request(self.rpc_provider.signer_address().clone()),
        )
        .into_pending(&self.rpc_provider)
        .await
        {
            Ok(pending_tx) => {
                log::info!("Submitted pending tx for {:?}: {:?}", address, pending_tx);

                // Tx was sent and is now pending - await final confirm or revert async
                let rpc_provider = Arc::clone(&self.rpc_provider);
                let trade_positions = Arc::clone(&self.trade_positions);

                tokio::spawn(async move {
                    match pending_tx.into_trade_metadata(&rpc_provider).await {
                        Ok(committed_trade) => {
                            // Add the committed position to the store
                            {
                                let mut trade_positions = trade_positions.write().unwrap();
                                trade_positions.insert(address, Trade::Open(committed_trade));
                            }
                        }
                        Err(err) => {
                            // Tx committed but we failed to convert to a committed trade
                            {
                                let mut trade_positions = trade_positions.write().unwrap();
                                trade_positions.remove(&address);
                            }

                            log::error!(
                                "failed to create committed_trade for {:?}: {:?}",
                                address,
                                err
                            );
                        }
                    }
                });

                Ok(())
            }
            Err(err) => {
                log::error!("Failed to submit pending tx for {:?}: {:?}", address, err);

                // Tx failed to send - remove the pending position from the store
                {
                    let mut trade_positions = self.trade_positions.write().unwrap();
                    trade_positions.remove(&address);
                }
                Err(err)
            }
        }
    }
}
