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

    pub fn set_active(&mut self, trade: Option<Trade<P>>) -> Option<Trade<P>> {
        std::mem::replace(&mut self.active, trade)
    }

    pub fn active(&self) -> &Option<Trade<P>> {
        &self.active
    }

    pub fn closed(&self) -> &Vec<(TradeMetadata<P>, TradeMetadata<P>)> {
        &self.closed
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
    pub fn set_active(
        &self,
        address: &Address,
        trade: Option<Trade<P>>,
    ) -> Result<Option<Trade<P>>> {
        println!("setting active for {:?}", address);
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

impl<P> TradeController<P>
where
    P: ParseableTrade,
{
    pub fn new(rpc_provider: Arc<RpcProvider>) -> Self {
        TradeController {
            rpc_provider,
            trades: Trades::default(),
        }
    }

    pub fn trades(&self) -> &Trades<P> {
        &self.trades
    }

    async fn send_tx<F, R>(
        &self,
        request: R,
        rpc_provider: Arc<RpcProvider>,
        on_confirmed: F,
    ) -> Result<()>
    where
        R: TradeRequest<P>,
        F: FnOnce(Result<TradeMetadata<P>>) + Send + Sync + 'static,
    {
        if *config::IS_BACKTEST || cfg!(test) {
            let rpc_provider = rpc_provider.clone();
            tokio::spawn(async move {
                let metadata = request.as_backtest_trade_metadata(&rpc_provider).await;
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

    pub async fn close_position<R: TradeRequest<P>>(&self, close_trade_request: R) -> Result<()> {
        // ensure that an existing open trade exists for this pair, update it to pending close

        let open_trade = {
            let mut trades = self.trades.0.write().unwrap();
            let address_trades = trades
                .get_mut(close_trade_request.address())
                .ok_or_else(|| eyre!("No open trade for {}", close_trade_request.address()))?;

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
                    close_trade_request.address(),
                    Some(Trade::Open(open_trade.clone())),
                )?;

                Err(err)
            }
        }?;

        let address = close_trade_request.address().clone();
        let trades = self.trades.clone();

        match self
            .send_tx(
                close_trade_request,
                self.rpc_provider.clone(),
                move |res| match res {
                    Ok(committed_trade) => {
                        // Backtest success: update trade closed, clear active trade
                        if let Err(err) = trades.close(&address, open_trade, committed_trade) {
                            log::error!("Failed to close trade for {:?}: {:?}", address, err);
                        }
                    }
                    Err(err) => {
                        // Backtest failed: revert the pending close active state
                        if let Err(revert_err) =
                            trades.set_active(&address, Some(Trade::Open(open_trade)))
                        {
                            log::error!(
                        "Failed to revert pending close for {:?}: {:?}, original error: {:?}",
                        address,
                        revert_err,
                        err
                    );
                        } else {
                            log::error!("Failed to close trade for {:?}: {:?}", address, err);
                        }
                    }
                },
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                log::error!("Failed to submit pending tx for {:?}: {:?}", address, err);

                // Tx failed to send - revert the pending close
                self.trades
                    .set_active(&address, Some(Trade::Open(open_trade)))?;

                Err(err)
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

            match address_trades.active() {
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
                            log::error!("Failed to open trade for {:?}: {:?}", address, err);
                        }
                    }
                    Err(err) => {
                        if let Err(revert_err) = trades.set_active(&address, None) {
                            log::error!(
                            "Failed to revert pending open for {:?}: {:?}, original error: {:?}",
                            address,
                            revert_err,
                            err
                        );
                        } else {
                            log::error!("Failed to open trade for {:?}: {:?}", address, err);
                        }
                    }
                },
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                log::error!("Failed to submit pending tx for {:?}: {:?}", address, err);

                // Tx failed to send - remove the pending position from the store
                if let Err(err) = self.trades.set_active(&address, None) {
                    log::error!(
                        "Failed to remove pending trade for {:?}: {:?}",
                        address,
                        err
                    );
                }

                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Trade, TradeController};

    use crate::{
        config,
        indexer::ParseableTrade,
        rpc_provider::RpcProvider,
        trade_controller::{TradeMetadata, TradeRequest},
    };

    use eyre::{eyre, Result};
    use std::sync::{Arc, Mutex};
    use tokio::time::{sleep, Duration};

    use alloy::{
        primitives::{Address, BlockNumber, U256},
        rpc::types::eth::{Log, TransactionRequest},
    };

    #[derive(Copy, Clone, Debug)]
    struct MockTrade {}
    impl ParseableTrade for MockTrade {
        fn parse_from_log(
            _log: &Log,
            _logs: &Vec<Log>,
            _relative_log_idx: usize,
        ) -> Option<MockTrade> {
            Some(MockTrade {})
        }
    }

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

    impl TradeRequest<MockTrade> for MockTradeRequest {
        fn as_transaction_request(&self, _signer_address: Address) -> TransactionRequest {
            unimplemented!()
        }

        fn address(&self) -> &Address {
            &self.address
        }

        async fn trace(&self, _rpc_provider: &RpcProvider) -> Result<()> {
            Ok(())
        }

        async fn as_backtest_trade_metadata(
            &self,
            _rpc_provider: &RpcProvider,
        ) -> Result<TradeMetadata<MockTrade>> {
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
                        MockTrade {},
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
        let controller: TradeController<MockTrade> =
            TradeController::new(Arc::new(RpcProvider::new(&config::RPC_URL).await?));
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
        let controller: TradeController<MockTrade> =
            TradeController::new(Arc::new(RpcProvider::new(&config::RPC_URL).await?));

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

            println!("{:?}", active_trade);

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
        let controller: TradeController<MockTrade> =
            TradeController::new(Arc::new(RpcProvider::new(&config::RPC_URL).await?));
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
        let controller = TradeController::new(Arc::new(RpcProvider::new(&config::RPC_URL).await?));

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
