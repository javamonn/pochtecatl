use super::{AddressTrades, Trade, TradeControllerRequest, Trades, Transaction};
use crate::config;

use pochtecatl_db::NewBacktestClosedTradeModel;
use pochtecatl_primitives::{constants, RpcProvider, TradeMetadata};

use alloy::{network::Ethereum, providers::Provider, transports::Transport};

use eyre::{eyre, Result};
use std::sync::Arc;
use tracing::{error, info};

pub struct TradeController<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    rpc_provider: Arc<RpcProvider<T, P>>,
    trades: Trades,
}

impl<T, P> TradeController<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum>,
{
    pub fn new(rpc_provider: Arc<RpcProvider<T, P>>) -> Self {
        TradeController {
            rpc_provider,
            trades: Trades::default(),
        }
    }

    pub fn trades(&self) -> &Trades {
        &self.trades
    }

    pub fn pending_handle(&self) -> tokio::task::JoinHandle<()> {
        let trades = self.trades.clone();

        tokio::spawn(async move {
            loop {
                {
                    let trades = trades.0.read().unwrap();
                    // If no pending trades exist, break out of loop
                    if !trades.values().any(|trade| {
                        matches!(
                            trade.active(),
                            Some(Trade::PendingOpen) | Some(Trade::PendingClose)
                        )
                    }) {
                        break;
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(
                    constants::AVERAGE_BLOCK_TIME_SECONDS,
                ))
                .await;
            }
        })
    }

    pub fn insert_backtest_closed_trades(
        &self,
        tx: &rusqlite::Transaction,
        backtest_id: i64,
    ) -> Result<()> {
        let trades = self.trades.0.read().unwrap();

        for address_trades in trades.values() {
            for (open_trade, close_trade) in address_trades.closed() {
                NewBacktestClosedTradeModel::new(
                    backtest_id,
                    *close_trade.indexed_trade().pair_address(),
                    *close_trade.block_timestamp(),
                    serde_json::to_value(open_trade)?,
                    serde_json::to_value(close_trade)?,
                )
                .insert(tx)?;
            }
        }

        Ok(())
    }

    async fn send_tx<R, F>(
        &self,
        request: R,
        rpc_provider: Arc<RpcProvider<T, P>>,
        on_confirmed: F,
    ) -> Result<()>
    where
        R: TradeControllerRequest + Send + 'static,
        F: FnOnce(Result<TradeMetadata>) + Send + 'static,
    {
        if cfg!(test) {
            let rpc_provider = rpc_provider.clone();
            tokio::spawn(async move {
                on_confirmed(request.simulate_trade_request(&rpc_provider).await);
            });

            Ok(())
        } else if *config::IS_BACKTEST {
            on_confirmed(request.simulate_trade_request(&rpc_provider).await);
            Ok(())
        } else {
            let transaction_request = request
                .make_trade_transaction_request(&rpc_provider)
                .await?;
            Transaction::send(transaction_request, &rpc_provider)
                .await
                .map(|tx| {
                    let rpc_provider = rpc_provider.clone();
                    tokio::spawn(async move {
                        let metadata = tx
                            .into_trade_metadata(
                                request.op().clone(),
                                *request.token_address(),
                                &rpc_provider,
                            )
                            .await;
                        on_confirmed(metadata);
                    });
                })
        }
    }

    pub async fn close_position<R>(&self, close_trade_request: R) -> Result<()>
    where
        R: TradeControllerRequest + Send + 'static,
    {
        // ensure that an existing open trade exists for this token, update it to pending close

        let open_trade = {
            let mut trades = self.trades.0.write().unwrap();
            let address_trades = trades
                .get_mut(close_trade_request.token_address())
                .ok_or_else(|| {
                    eyre!("No open trade for {}", close_trade_request.token_address())
                })?;

            match address_trades.set_active(Some(Trade::PendingClose)) {
                Some(Trade::Open(open_trade)) => Ok(open_trade),
                existing => {
                    // Unexpected state, revert the active trade to whatever was there before and
                    // Err
                    address_trades.set_active(existing.clone());
                    Err(eyre!("Unexpected active trade state: {:?}", existing))
                }
            }
        }?;

        // Ensure the trade request is valid, otherwise revert the pending close
        match close_trade_request.trace(&self.rpc_provider).await {
            Ok(_) => Ok(()),
            Err(err) => {
                self.trades.set_active(
                    close_trade_request.token_address(),
                    Some(Trade::Open(open_trade.clone())),
                )?;

                Err(err)
            }
        }?;

        let address = close_trade_request.token_address().clone();
        let trades = self.trades.clone();
        let moved_open_trade = open_trade.clone();

        match self
            .send_tx(
                close_trade_request,
                self.rpc_provider.clone(),
                move |res| match res {
                    Ok(committed_trade) => {
                        info!(
                            token_address = address.to_string(),
                            tx_hash = committed_trade.tx_hash().to_string(),
                            "committed close trade"
                        );

                        // Backtest success: update trade closed, clear active trade
                        if let Err(err) = trades.close(&address, moved_open_trade, committed_trade)
                        {
                            error!(
                                address = address.to_string(),
                                "Failed to update trades for closed trade: {:?}", err
                            );
                        }
                    }
                    Err(err) => {
                        // Backtest failed: revert the pending close active state
                        if let Err(revert_err) =
                            trades.set_active(&address, Some(Trade::Open(moved_open_trade)))
                        {
                            error!(
                                address = address.to_string(),
                                "Failed to revert pending close: {:?}, original error: {:?}",
                                revert_err,
                                err
                            );
                        } else {
                            error!(
                                address = address.to_string(),
                                "Failed to close trade: {:?}", err
                            );
                        }
                    }
                },
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                error!(
                    address = address.to_string(),
                    "Failed to submit pending tx: {:?}", err
                );

                // Tx failed to send - revert the pending close
                self.trades
                    .set_active(&address, Some(Trade::Open(open_trade)))?;

                Err(err)
            }
        }
    }

    pub async fn open_position<R>(&self, open_position_request: R) -> Result<()>
    where
        R: TradeControllerRequest + Send + 'static,
    {
        // ensure that we do not already have a position for this token and add
        // the position to the store
        {
            let mut trades = self.trades.0.write().unwrap();
            let address_trades = trades
                .entry(open_position_request.token_address().clone())
                .or_insert_with(|| AddressTrades::default());

            match address_trades.active() {
                None => { /* expected state */ }
                Some(_) => {
                    return Err(eyre!(
                        "Position already exists for token {}",
                        open_position_request.token_address()
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
                    .set_active(open_position_request.token_address(), None);
            })?;

        let token_address = open_position_request.token_address().clone();
        let trades = self.trades.clone();

        match self
            .send_tx(
                open_position_request,
                self.rpc_provider.clone(),
                move |res| match res {
                    Ok(committed_trade) => {
                        info!(
                            token_address = token_address.to_string(),
                            tx_hash = committed_trade.tx_hash().to_string(),
                            "committed open trade"
                        );

                        if let Err(err) =
                            trades.set_active(&token_address, Some(Trade::Open(committed_trade)))
                        {
                            error!(
                                token_address = token_address.to_string(),
                                "Failed to update trades for open trade: {:?}", err
                            );
                        }
                    }
                    Err(err) => {
                        if let Err(revert_err) = trades.set_active(&token_address, None) {
                            error!(
                                token_address = token_address.to_string(),
                                "Failed to revert pending open: {:?}, original error: {:?}",
                                revert_err,
                                err
                            );
                        } else {
                            error!(
                                token_address = token_address.to_string(),
                                "Failed to open trade: {:?}", err
                            );
                        }
                    }
                },
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                error!(
                    token_address = token_address.to_string(),
                    "Failed to submit pending tx: {:?}", err
                );

                // Tx failed to send - remove the pending position from the store
                if let Err(err) = self.trades.set_active(&token_address, None) {
                    error!(
                        token_address = token_address.to_string(),
                        "Failed to remove pending trade: {:?}", err
                    );
                }

                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TradeController;

    use crate::{
        config,
        trade_controller::{Trade, TradeControllerRequest},
    };

    use pochtecatl_primitives::{
        new_http_signer_provider, IndexedTrade, RpcProvider, TradeMetadata, TradeRequestOp,
        UniswapV2IndexedTrade,
    };

    use eyre::{eyre, Result};
    use hex_literal::hex;
    use std::sync::{Arc, Mutex};
    use tokio::time::{sleep, Duration};

    use alloy::{
        network::Ethereum,
        primitives::{Address, BlockNumber, TxHash, B256, U256},
        providers::Provider,
        rpc::types::eth::TransactionRequest,
        transports::Transport,
    };

    struct MockTradeRequest {
        token_address: Address,
        block_number: BlockNumber,
        confirmed_lock: Arc<Mutex<()>>,
        should_revert: bool,
    }

    impl MockTradeRequest {
        fn new(token_address: Address, block_number: BlockNumber, should_revert: bool) -> Self {
            Self {
                token_address,
                block_number,
                confirmed_lock: Arc::new(Mutex::new(())),
                should_revert,
            }
        }

        fn confirmed_lock(&self) -> Arc<Mutex<()>> {
            Arc::clone(&self.confirmed_lock)
        }
    }

    impl TradeControllerRequest for MockTradeRequest {
        fn token_address(&self) -> &Address {
            &self.token_address
        }

        fn op(&self) -> &TradeRequestOp {
            &TradeRequestOp::Open
        }

        async fn make_trade_transaction_request<T, P>(
            &self,
            _rpc_provider: &RpcProvider<T, P>,
        ) -> Result<TransactionRequest>
        where
            T: Transport + Clone,
            P: Provider<T, Ethereum> + 'static,
        {
            unimplemented!()
        }

        async fn trace<T, P>(&self, _rpc_provider: &RpcProvider<T, P>) -> Result<()>
        where
            T: Transport + Clone,
            P: Provider<T, Ethereum> + 'static,
        {
            Ok(())
        }

        async fn simulate_trade_request<T, P>(
            &self,
            _rpc_provider: &RpcProvider<T, P>,
        ) -> Result<TradeMetadata>
        where
            T: Transport + Clone,
            P: Provider<T, Ethereum> + 'static,
        {
            let confirmed_lock = Arc::clone(&self.confirmed_lock);
            let block_number = self.block_number;
            let should_revert = self.should_revert;
            tokio::spawn(async move {
                let _hold = confirmed_lock.lock().unwrap();
                if should_revert {
                    Err(eyre!("should_revert is true"))
                } else {
                    Ok(TradeMetadata::new(
                        TxHash::ZERO,
                        block_number,
                        0,
                        TradeRequestOp::Open,
                        Address::ZERO,
                        U256::ZERO,
                        IndexedTrade::UniswapV2(UniswapV2IndexedTrade::new(
                            Address::ZERO,
                            U256::ZERO,
                            U256::ZERO,
                            U256::ZERO,
                            U256::ZERO,
                            U256::ZERO,
                            U256::ZERO,
                            Address::ZERO,
                        )),
                    ))
                }
            })
            .await
            .map_err(|err| err.into())
            .and_then(|inner| inner)
        }
    }

    #[tokio::test]
    async fn test_open_position() -> Result<()> {
        let controller = TradeController::new(Arc::new(
            new_http_signer_provider(
                url::Url::parse(config::RPC_URL.as_str())?,
                &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
                None,
                true,
            )
            .await?,
        ));
        let req = MockTradeRequest::new(Address::ZERO, 0, false);
        let confirmed_lock = req.confirmed_lock();

        // Test the pending state
        {
            // Hold the lock so that we can test pending state
            let _hold = confirmed_lock.lock().unwrap();

            controller.open_position(req).await?;

            let trades = controller.trades().0.read().unwrap();
            let active_trade = trades
                .get(&Address::ZERO)
                .and_then(|trades| trades.active().as_ref())
                .ok_or_else(|| eyre!("Expected active trade"))?;

            assert!(matches!(active_trade, Trade::PendingOpen));
        }

        // Wait for trade confirmation to unlock
        sleep(Duration::from_millis(100)).await;

        // Lock dropped, trade should be active once we can re-acquire the lock
        {
            let _hold = confirmed_lock.lock().unwrap();
            let trades = controller.trades().0.read().unwrap();

            let active_trade = trades
                .get(&Address::ZERO)
                .and_then(|trades| trades.active().as_ref())
                .ok_or_else(|| eyre!("Expected active trade"))?;

            match active_trade {
                Trade::Open(open_trade) => {
                    assert_eq!(*open_trade.block_number(), 0);
                    Ok(())
                }
                _ => Err(eyre!("Expected open trade")),
            }
        }
    }

    #[tokio::test]
    async fn test_close_position() -> Result<()> {
        let controller = TradeController::new(Arc::new(
            new_http_signer_provider(
                url::Url::parse(config::RPC_URL.as_str())?,
                &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
                None,
                true,
            )
            .await?,
        ));

        // open a position
        controller
            .open_position(MockTradeRequest::new(Address::ZERO, 0, false))
            .await?;
        // Wait for trade confirmation
        sleep(Duration::from_millis(100)).await;

        let close_req = MockTradeRequest::new(Address::ZERO, 1, false);
        let confirmed_lock = close_req.confirmed_lock();

        // Test the pending close state
        {
            // Hold the lock so that we can test pending state
            let _hold = confirmed_lock.lock().unwrap();

            // close the position
            controller.close_position(close_req).await?;

            let trades = controller.trades().0.read().unwrap();
            let active_trade = trades
                .get(&Address::ZERO)
                .and_then(|trades| trades.active().as_ref())
                .ok_or_else(|| eyre!("Expected active trade"))?;

            assert!(matches!(active_trade, Trade::PendingClose));
        }

        // Wait for trade confirmation to unlock
        sleep(Duration::from_millis(100)).await;

        // Lock dropped, trade should be confirmed and closed once we can re-acquire the
        // lock
        {
            let _hold = confirmed_lock.lock().unwrap();
            let trades = controller.trades().0.read().unwrap();

            match trades
                .get(&Address::ZERO)
                .and_then(|trades| trades.closed().first())
            {
                Some((open_trade, close_trade)) => {
                    assert_eq!(*open_trade.block_number(), 0);
                    assert_eq!(*close_trade.block_number(), 1);
                    Ok(())
                }
                _ => Err(eyre!("Expected closed trade")),
            }
        }
    }

    #[tokio::test]
    async fn test_open_position_revert() -> Result<()> {
        let controller = TradeController::new(Arc::new(
            new_http_signer_provider(
                url::Url::parse(config::RPC_URL.as_str())?,
                &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
                None,
                true,
            )
            .await?,
        ));
        let open_req = MockTradeRequest::new(Address::ZERO, 0, true);
        let confirmed_lock = open_req.confirmed_lock();

        controller.open_position(open_req).await?;

        // Wait for trade revert
        sleep(Duration::from_millis(100)).await;

        // Trade should be reverted and not set in active
        {
            let _lock = confirmed_lock.lock().unwrap();
            let trades = controller.trades().0.read().unwrap();
            let active_trade = trades
                .get(&Address::ZERO)
                .and_then(|trades| trades.active().as_ref());

            assert!(active_trade.is_none());
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_close_position_revert() -> Result<()> {
        let controller = TradeController::new(Arc::new(
            new_http_signer_provider(
                url::Url::parse(config::RPC_URL.as_str())?,
                &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
                None,
                true,
            )
            .await?,
        ));

        // Open a position to test close later
        {
            let open_req = MockTradeRequest::new(Address::ZERO, 0, false);
            let confirmed_lock = open_req.confirmed_lock();

            controller.open_position(open_req).await?;

            // Wait for trade confirmation
            sleep(Duration::from_millis(100)).await;

            let _lock = confirmed_lock.lock().unwrap();
        }

        let close_req = MockTradeRequest::new(Address::ZERO, 1, true);
        let confirmed_lock = close_req.confirmed_lock();
        controller.close_position(close_req).await?;

        // Wait for trade revert
        sleep(Duration::from_millis(100)).await;

        // Trade should be reverted and previous Open state should be in active
        {
            let _lock = confirmed_lock.lock().unwrap();
            let trades = controller.trades().0.read().unwrap();

            match trades
                .get(&Address::ZERO)
                .and_then(|trades| trades.active().as_ref())
            {
                Some(Trade::Open(open_trade)) => {
                    assert_eq!(*open_trade.block_number(), 0);
                    Ok(())
                }
                _ => Err(eyre!("Expected open trade")),
            }
        }
    }
}
