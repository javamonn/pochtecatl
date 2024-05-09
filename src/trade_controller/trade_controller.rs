use super::{AddressTrades, Trade, TradeMetadata, TradeRequestIntent, Trades, Transaction};
use crate::{config, db::NewBacktestClosedTradeModel, providers::RpcProvider};

use alloy::{
    network::Ethereum, primitives::Address, providers::Provider,
    rpc::types::eth::TransactionRequest, transports::Transport,
};

use eyre::{eyre, Result};
use std::sync::Arc;
use tracing::error;

pub trait TradeControllerRequest {
    fn pair_address(&self) -> &Address;
    fn as_transaction_request(&self, signer_address: Address) -> TransactionRequest;

    async fn trace<T, P>(&self, rpc_provider: &RpcProvider<T, P>) -> Result<()>
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;

    fn estimate_trade_metadata<T, P>(
        &self,
        rpc_provider: &RpcProvider<T, P>,
    ) -> impl std::future::Future<Output = Result<TradeMetadata>> + Send
    where
        T: Transport + Clone,
        P: Provider<T, Ethereum> + 'static;
}

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
                    *config::AVERAGE_BLOCK_TIME_SECONDS,
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
                    open_trade.clone(),
                    close_trade.clone(),
                )
                .insert(tx)?;
            }
        }

        Ok(())
    }

    async fn send_tx<F, R>(
        &self,
        request: R,
        rpc_provider: Arc<RpcProvider<T, P>>,
        on_confirmed: F,
    ) -> Result<()>
    where
        R: TradeControllerRequest + Send + 'static,
        F: FnOnce(Result<TradeMetadata>) + Send + 'static,
    {
        if *config::IS_BACKTEST || cfg!(test) {
            let rpc_provider = rpc_provider.clone();
            tokio::spawn(async move {
                let metadata = request.estimate_trade_metadata(&rpc_provider).await;
                on_confirmed(metadata);
            });

            Ok(())
        } else {
            Transaction::send(
                request.as_transaction_request(rpc_provider.signer_address().clone()),
                &rpc_provider,
            )
            .await
            .map(|tx| {
                let rpc_provider = rpc_provider.clone();
                tokio::spawn(async move {
                    let metadata = tx.into_trade_metadata(&rpc_provider).await;
                    on_confirmed(metadata);
                });
            })
        }
    }

    pub async fn close_position<R>(&self, close_trade_request: R) -> Result<()>
    where
        R: TradeControllerRequest + Send + 'static,
    {
        // ensure that an existing open trade exists for this pair, update it to pending close

        let open_trade = {
            let mut trades = self.trades.0.write().unwrap();
            let address_trades = trades
                .get_mut(close_trade_request.pair_address())
                .ok_or_else(|| eyre!("No open trade for {}", close_trade_request.pair_address()))?;

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
                    close_trade_request.pair_address(),
                    Some(Trade::Open(open_trade.clone())),
                )?;

                Err(err)
            }
        }?;

        let address = close_trade_request.pair_address().clone();
        let trades = self.trades.clone();

        match self
            .send_tx(
                close_trade_request,
                self.rpc_provider.clone(),
                move |res| match res {
                    Ok(committed_trade) => {
                        // Backtest success: update trade closed, clear active trade
                        if let Err(err) = trades.close(&address, open_trade, committed_trade) {
                            error!(
                                address = address.to_string(),
                                "Failed to close trade: {:?}", err
                            );
                        }
                    }
                    Err(err) => {
                        // Backtest failed: revert the pending close active state
                        if let Err(revert_err) =
                            trades.set_active(&address, Some(Trade::Open(open_trade)))
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
        // ensure that we do not already have a position for this pair and add
        // the position to the store
        {
            let mut trades = self.trades.0.write().unwrap();
            let address_trades = trades
                .entry(open_position_request.pair_address().clone())
                .or_insert_with(|| AddressTrades::default());

            match address_trades.active() {
                None => { /* expected state */ }
                Some(_) => {
                    return Err(eyre!(
                        "Position already exists for pair {}",
                        open_position_request.pair_address()
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
                    .set_active(open_position_request.pair_address(), None);
            })?;

        let address = open_position_request.pair_address().clone();
        let trades = self.trades.clone();

        match self
            .send_tx(
                open_position_request,
                self.rpc_provider.clone(),
                move |res| match res {
                    Ok(committed_trade) => {
                        if let Err(err) =
                            trades.set_active(&address, Some(Trade::Open(committed_trade)))
                        {
                            error!(
                                address = address.to_string(),
                                "Failed to open trade: {:?}", err
                            );
                        }
                    }
                    Err(err) => {
                        if let Err(revert_err) = trades.set_active(&address, None) {
                            error!(
                                address = address.to_string(),
                                "Failed to revert pending open: {:?}, original error: {:?}",
                                revert_err,
                                err
                            );
                        } else {
                            error!(
                                address = address.to_string(),
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
                    address = address.to_string(),
                    "Failed to submit pending tx: {:?}", err
                );

                // Tx failed to send - remove the pending position from the store
                if let Err(err) = self.trades.set_active(&address, None) {
                    error!(
                        address = address.to_string(),
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
    use super::{TradeController, TradeControllerRequest};

    use crate::{
        config,
        primitives::UniswapV2PairTrade,
        providers::{rpc_provider::new_http_signer_provider, RpcProvider},
        trade_controller::{ParsedTrade, Trade, TradeMetadata},
    };

    use eyre::{eyre, Result};
    use std::sync::{Arc, Mutex};
    use tokio::time::{sleep, Duration};

    use alloy::{
        network::Ethereum,
        primitives::{Address, BlockNumber, U256},
        providers::Provider,
        rpc::types::eth::TransactionRequest,
        transports::Transport,
    };

    struct MockTradeRequest {
        address: Address,
        block_number: BlockNumber,
        confirmed_lock: Arc<Mutex<()>>,
        should_revert: bool,
    }

    impl MockTradeRequest {
        fn new(address: Address, block_number: BlockNumber, should_revert: bool) -> Self {
            Self {
                address,
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
        fn pair_address(&self) -> &Address {
            &self.address
        }

        fn as_transaction_request(&self, _signer_address: Address) -> TransactionRequest {
            unimplemented!()
        }

        async fn trace<T, P>(&self, _rpc_provider: &RpcProvider<T, P>) -> Result<()>
        where
            T: Transport + Clone,
            P: Provider<T, Ethereum> + 'static,
        {
            Ok(())
        }

        async fn estimate_trade_metadata<T, P>(
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
                        block_number,
                        0,
                        U256::ZERO,
                        ParsedTrade::UniswapV2PairTrade(UniswapV2PairTrade::new(
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
            new_http_signer_provider(&config::RPC_URL, None).await?,
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
            new_http_signer_provider(&config::RPC_URL, None).await?,
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
            new_http_signer_provider(&config::RPC_URL, None).await?,
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
            new_http_signer_provider(&config::RPC_URL, None).await?,
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
