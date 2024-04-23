use super::{StrategyExecutor, UniswapV2MomentumStrategy, UniswapV2Strategy};

use crate::{
    indexer::{IndexedBlockMessage, ResolutionTimestamp, TimePriceBarStore, UniswapV2PairTrade},
    trade_controller::{
        Trade, TradeController, UniswapV2CloseTradeRequest, UniswapV2OpenTradeRequest,
    },
};

use eyre::{eyre, Result};
use std::sync::{mpsc::Receiver, Arc};
use tokio::task::{JoinHandle, JoinSet};

pub struct UniswapV2StrategyExecuctor<S: UniswapV2Strategy> {
    strategy: Arc<S>,
    exec_handle: Option<JoinHandle<Result<()>>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
    trade_controller: Arc<TradeController<UniswapV2PairTrade>>,
}

impl<S: UniswapV2Strategy> UniswapV2StrategyExecuctor<S> {
    pub fn new(
        time_price_bar_store: Arc<TimePriceBarStore>,
        trade_controller: Arc<TradeController<UniswapV2PairTrade>>,
        strategy: S,
    ) -> Self {
        Self {
            exec_handle: None,
            time_price_bar_store,
            trade_controller,
            strategy: Arc::new(strategy),
        }
    }

    pub fn with_momentum_strategy(
        time_price_bar_store: Arc<TimePriceBarStore>,
        trade_controller: Arc<TradeController<UniswapV2PairTrade>>,
    ) -> UniswapV2StrategyExecuctor<UniswapV2MomentumStrategy> {
        UniswapV2StrategyExecuctor::new(
            time_price_bar_store,
            trade_controller,
            UniswapV2MomentumStrategy::new(),
        )
    }
}

impl<S: UniswapV2Strategy + Send + Sync + 'static> StrategyExecutor
    for UniswapV2StrategyExecuctor<S>
{
    fn exec(&mut self, indexed_block_message_receiver: Receiver<IndexedBlockMessage>) {
        let time_price_bar_store = Arc::clone(&self.time_price_bar_store);
        let trade_controller = Arc::clone(&self.trade_controller);
        let strategy = Arc::clone(&self.strategy);

        let exec_handle = tokio::spawn(async move {
            while let Ok(indexed_block_message) = indexed_block_message_receiver.recv() {
                let trade_controller = Arc::clone(&trade_controller);

                handle_indexed_block_message(
                    indexed_block_message,
                    strategy.as_ref(),
                    time_price_bar_store.as_ref(),
                    trade_controller,
                )
                .await?;
            }

            Ok(())
        });

        self.exec_handle = Some(exec_handle);
    }

    async fn join(self) -> Result<()> {
        if let Some(exec_handle) = self.exec_handle {
            exec_handle.await??;
        }

        Ok(())
    }
}

async fn handle_indexed_block_message<S: UniswapV2Strategy>(
    indexed_block_message: IndexedBlockMessage,
    strategy: &S,
    time_price_bar_store: &TimePriceBarStore,
    trade_controller: Arc<TradeController<UniswapV2PairTrade>>,
) -> Result<()> {
    let mut pending_tx_tasks = JoinSet::new();

    // Execute core strategy logic
    {
        let pair_trades = trade_controller.trade_positions().read().unwrap();
        let time_price_bars = time_price_bar_store.time_price_bars().read().unwrap();

        let resolution = time_price_bar_store.resolution();
        let timestamp =
            ResolutionTimestamp::from_timestamp(indexed_block_message.block_timestamp, &resolution);

        for uniswap_v2_pair in indexed_block_message.uniswap_v2_pairs.into_iter() {
            let trade = pair_trades.get(&uniswap_v2_pair.pair_address);

            match trade {
                None => {
                    match strategy.should_open_position(
                        &uniswap_v2_pair,
                        &timestamp,
                        &time_price_bars,
                    ) {
                        Ok(()) => {
                            let open_position_request = UniswapV2OpenTradeRequest::new(
                                uniswap_v2_pair.pair_address,
                                uniswap_v2_pair.token_address,
                                uniswap_v2_pair.weth_reserve,
                                uniswap_v2_pair.token_reserve,
                                indexed_block_message.block_number,
                                indexed_block_message.block_timestamp,
                            );
                            let trade_controller = Arc::clone(&trade_controller);

                            pending_tx_tasks.spawn(async move {
                                let _ = trade_controller.open_position(open_position_request).await;

                                log::info!(
                                    block_number = indexed_block_message.block_number,
                                    pair_address = uniswap_v2_pair.pair_address.to_string();
                                    "executed opened position"
                                );
                            });
                        }
                        Err(err) => {
                            log::debug!(
                                block_number = indexed_block_message.block_number,
                                pair_address = uniswap_v2_pair.pair_address.to_string();
                                "Skipping open position: {:?}", err
                            );
                        }
                    }
                }
                Some(Trade::Open(open_trade_metadata)) => match strategy.should_close_position(
                    &uniswap_v2_pair,
                    &timestamp,
                    &resolution,
                    &open_trade_metadata,
                    &time_price_bars,
                ) {
                    Ok(()) => {
                        let close_position_request = UniswapV2CloseTradeRequest::new();
                        let trade_controller = Arc::clone(&trade_controller);

                        pending_tx_tasks.spawn(async move {
                            let _ = trade_controller
                                .close_position(close_position_request)
                                .await;

                            log::info!(
                                block_number = indexed_block_message.block_number,
                                pair_address = uniswap_v2_pair.pair_address.to_string();
                                "executed close position"
                            );
                        });
                    }
                    Err(err) => {
                        log::debug!(
                            block_number = indexed_block_message.block_number,
                            pair_address = uniswap_v2_pair.pair_address.to_string();
                            "Skipping close position: {:?}", err
                        );
                    }
                },
                _ => { /* noop */ }
            };
        }
    }

    // Await completion of all pending tx submissions
    while let Some(pending_tx_result) = pending_tx_tasks.join_next().await {
        let _ = pending_tx_result.inspect_err(|err| {
            log::error!(
                block_number = indexed_block_message.block_number;
                "join set execution error: {:?}",
                err
            );
        });
    }

    // If message included an ack, trigger it
    if let Some(ack) = indexed_block_message.ack {
        ack.send(())
            .map_err(|e| eyre!("Failed to send ack: {:?}", e))?;
    }

    Ok(())
}
