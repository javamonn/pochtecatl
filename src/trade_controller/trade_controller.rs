use crate::{config, indexer::ParseableTrade, rpc_provider::RpcProvider};

use super::{Trade, TradeMetadata, TradeRequest, Transaction};

use alloy::primitives::Address;

use eyre::{eyre, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};

pub struct AddressTrades<P: ParseableTrade> {
    active: Option<Trade<P>>,
    closed: Vec<(TradeMetadata<P>, TradeMetadata<P>)>,
}

impl<P: ParseableTrade> AddressTrades<P> {
    pub fn close(&mut self, open_trade: TradeMetadata<P>, committed_trade: TradeMetadata<P>) {
        self.active = None;
        self.closed.push((open_trade, committed_trade));
    }

    pub fn set_active(&mut self, trade: Option<Trade<P>>) {
        self.active = trade
    }

    pub fn active(&self) -> &Option<Trade<P>> {
        &self.active
    }
}

impl<P: ParseableTrade> Default for AddressTrades<P> {
    fn default() -> Self {
        Self {
            active: None,
            closed: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct Trades<P: ParseableTrade>(pub Arc<RwLock<FnvHashMap<Address, AddressTrades<P>>>>);
impl<P: ParseableTrade> Default for Trades<P> {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(FnvHashMap::default())))
    }
}

impl<P: ParseableTrade> Trades<P> {
    pub fn set_active(&self, address: &Address, trade: Option<Trade<P>>) -> Result<()> {
        self.0
            .write()
            .unwrap()
            .get_mut(address)
            .map(|trades| trades.set_active(trade))
            .ok_or_else(|| eyre!("Missing trade for {}", address))
    }

    pub fn close(
        &self,
        address: &Address,
        open_trade: TradeMetadata<P>,
        committed_trade: TradeMetadata<P>,
    ) -> Result<()> {
        self.0
            .write()
            .unwrap()
            .get_mut(address)
            .map(|trades| trades.close(open_trade, committed_trade))
            .ok_or_else(|| eyre!("Missing trade for {}", address))
    }
}

pub struct TradeController<P: ParseableTrade> {
    rpc_provider: Arc<RpcProvider>,
    trades: Trades<P>,
}

impl<P: ParseableTrade + Send + Sync + 'static> TradeController<P> {
    pub fn new(rpc_provider: Arc<RpcProvider>) -> Self {
        Self {
            rpc_provider,
            trades: Trades::default(),
        }
    }

    pub fn trades(&self) -> &Trades<P> {
        &self.trades
    }

    pub async fn close_position<R: TradeRequest<P>>(&self, close_trade_request: R) -> Result<()> {
        // ensure that an existing open trade exists for this pair, update it to pending close
        let open_trade = {
            match self
                .trades
                .0
                .write()
                .unwrap()
                .get_mut(close_trade_request.address())
                .and_then(|trades| trades.active)
            {
                Some(ref mut trade) => {
                    let open_trade = match trade {
                        Trade::Open(open_trade) => Ok(open_trade.clone()),
                        _ => Err(eyre!(
                            "Trade is pending close for {}",
                            close_trade_request.address()
                        )),
                    };
                    *trade = Trade::PendingClose;
                    open_trade
                }
                _ => Err(eyre!("No open trade for {}", close_trade_request.address())),
            }
        }?;

        // Ensure the trade request is valid, otherwise revert the pending close
        match close_trade_request.trace(&self.rpc_provider).await {
            Ok(_) => Ok(()),
            Err(err) => {
                self.trades.set_active(
                    close_trade_request.address(),
                    Some(Trade::Open(open_trade.clone())),
                )?;

                Err(err)
            }
        }?;

        let address = close_trade_request.address().clone();

        if *config::IS_BACKFILL {
            match close_trade_request
                .as_backtest_trade_metadata(&self.rpc_provider)
                .await
            {
                Ok(committed_trade) => {
                    // Backtest success: update trade closed, clear active trade
                    self.trades.close(
                        close_trade_request.address(),
                        open_trade,
                        committed_trade,
                    )?;

                    Ok(())
                }
                Err(err) => {
                    // Backtest failed: revert the pending close active state
                    self.trades
                        .set_active(close_trade_request.address(), Some(Trade::Open(open_trade)))?;

                    Err(err)
                }
            }
        } else {
            // Live Tx
            //
            // Txs are committed in two steps:
            // 1. first as a pending tx to the network, which this method awaits the completion of
            // 2. second as a committed trade, which is awaited in a separate async block
            match Transaction::send(
                close_trade_request
                    .as_transaction_request(self.rpc_provider.signer_address().clone()),
                &self.rpc_provider,
            )
            .await
            {
                Ok(tx) => {
                    log::info!("Submitted pending tx for {:?}: {:?}", address, tx);

                    // Tx was sent and is now pending - await final confirm or revert async
                    let rpc_provider = Arc::clone(&self.rpc_provider);
                    let trades = self.trades.clone();

                    tokio::spawn(async move {
                        match tx.into_trade_metadata(&rpc_provider).await {
                            Ok(committed_trade) => {
                                // Commit: update trade closed, clear active trade
                                trades.close(&address, open_trade, committed_trade)
                            }
                            Err(err) => {
                                // Commit, but failed convert: revert the pending close active state
                                log::error!(
                                    "failed to create committed_trade for {:?}: {:?}",
                                    address,
                                    err
                                );

                                trades.set_active(&address, Some(Trade::Open(open_trade)))
                            }
                        }
                    });

                    Ok(())
                }
                Err(err) => {
                    log::error!("Failed to submit pending tx for {:?}: {:?}", address, err);

                    // Tx failed to send - revert the pending close
                    self.trades
                        .set_active(&address, Some(Trade::Open(open_trade)))?;

                    Err(err)
                }
            }
        }
    }

    pub async fn open_position<R: TradeRequest<P>>(&self, open_position_request: R) -> Result<()> {
        // ensure that we do not already have a position for this pair and add
        // the position to the store
        {
            let mut trades = self.trades.0.write().unwrap();
            let address_trades = trades
                .entry(open_position_request.address().clone())
                .or_insert_with(|| AddressTrades::default());

            match &address_trades.active {
                None => { /* expected state */ }
                Some(_) => {
                    return Err(eyre!(
                        "Position already exists for pair {}",
                        open_position_request.address()
                    ));
                }
            };

            address_trades.set_active(Some(Trade::PendingOpen));
        }

        // Ensure the position is valid, otherwise remove the pending position from
        // the store
        open_position_request
            .trace(&self.rpc_provider)
            .await
            .inspect_err(|_| {
                let _ = self
                    .trades
                    .set_active(open_position_request.address(), None);
            })?;

        let address = open_position_request.address().clone();

        if *config::IS_BACKFILL {
            // Backfill Tx
            match open_position_request
                .as_backtest_trade_metadata(&self.rpc_provider)
                .await
            {
                Ok(committed_trade) => {
                    // Backtest success: Add the committed position to the store
                    self.trades
                        .set_active(&address, Some(Trade::Open(committed_trade)))
                }
                Err(err) => {
                    // Backtest failed: remove the pending position from the store
                    self.trades.set_active(&address, None)?;

                    Err(err)
                }
            }
        } else {
            // Live Tx
            //
            // Txs are committed in two steps:
            // 1. first as a pending tx to the network, which this method awaits the completion of
            // 2. second as a committed trade, which is awaited in a separate async block
            match Transaction::send(
                open_position_request
                    .as_transaction_request(self.rpc_provider.signer_address().clone()),
                &self.rpc_provider,
            )
            .await
            {
                Ok(tx) => {
                    log::info!("Submitted pending tx for {:?}: {:?}", address, tx);

                    // Tx was sent and is now pending - await final confirm or revert async
                    let rpc_provider = Arc::clone(&self.rpc_provider);
                    let trades = self.trades.clone();

                    tokio::spawn(async move {
                        match tx.into_trade_metadata(&rpc_provider).await {
                            Ok(committed_trade) => {
                                // Add the committed position to the store
                                trades.set_active(&address, Some(Trade::Open(committed_trade)))
                            }
                            Err(err) => {
                                // Tx committed but we failed to convert to a committed trade
                                log::error!(
                                    "failed to create committed_trade for {:?}: {:?}",
                                    address,
                                    err
                                );

                                trades.set_active(&address, None)
                            }
                        }
                    });

                    Ok(())
                }
                Err(err) => {
                    log::error!("Failed to submit pending tx for {:?}: {:?}", address, err);

                    // Tx failed to send - remove the pending position from the store
                    self.trades.set_active(&address, None)?;

                    Err(err)
                }
            }
        }
    }
}
