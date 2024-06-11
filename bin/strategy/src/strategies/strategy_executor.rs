use super::Strategy;

use crate::{
    indexer::{TimePriceBarBlockMessage, TimePriceBarController},
    trade_controller::{Trade, TradeController, TradeRequest},
};

use pochtecatl_primitives::TradeRequestOp;

use alloy::{network::Ethereum, providers::Provider, transports::Transport};
use chrono::DateTime;
use eyre::Result;
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, error, info, instrument};

pub struct StrategyExecutor<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    strategy: Box<dyn Strategy>,
    trade_controller: Arc<TradeController<T, P>>,
}

impl<T, P> StrategyExecutor<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum>,
{
    pub fn new(trade_controller: Arc<TradeController<T, P>>, strategy: Box<dyn Strategy>) -> Self {
        Self {
            trade_controller,
            strategy,
        }
    }

    #[instrument(skip_all)]
    pub async fn on_time_price_bar_block_message(
        &self,
        block_message: TimePriceBarBlockMessage,
        time_price_bar_controller: &TimePriceBarController,
    ) -> Result<()> {
        let mut pending_tx_tasks = JoinSet::new();

        // Execute core strategy logic
        {
            let trades = self.trade_controller.trades().0.read().unwrap();
            let time_price_bars = time_price_bar_controller.time_price_bars().read().unwrap();
            for pair in block_message.updated_pairs.into_iter() {
                // note that time price bars are by pair, trades are by token
                let pair_time_price_bars =
                    time_price_bars.get(pair.address()).unwrap_or_else(|| {
                        panic!(
                            "missing time price bars for pair: {}",
                            pair.address().to_string()
                        )
                    });

                let address_trades = trades.get(pair.token_address());
                let active_trade = address_trades.and_then(|t| t.active().as_ref());
                let last_closed_trade = address_trades
                    .and_then(|t| t.closed().last())
                    .map(|(_, close)| close);

                let trade_request = match active_trade {
                    None => self
                        .strategy
                        .should_open_position(
                            &pair_time_price_bars,
                            block_message.block_timestamp,
                            last_closed_trade,
                        )
                        .inspect_err(|err| {
                            debug!(
                                block_number = block_message.block_number,
                                datetime = DateTime::from_timestamp(
                                    block_message.block_timestamp as i64,
                                    0
                                )
                                .unwrap()
                                .to_rfc2822(),
                                pair_address = pair.address().to_string(),
                                "skipped open trade execution: {:?}",
                                err
                            );
                        })
                        .map(|_| {
                            TradeRequest::open(
                                block_message.block_number,
                                block_message.block_timestamp,
                                pair,
                            )
                        }),
                    Some(Trade::Open(open_trade_metadata)) => self
                        .strategy
                        .should_close_position(
                            &pair_time_price_bars,
                            block_message.block_timestamp,
                            &open_trade_metadata,
                        )
                        .inspect_err(|err| {
                            debug!(
                                block_number = block_message.block_number,
                                datetime = DateTime::from_timestamp(
                                    block_message.block_timestamp as i64,
                                    0
                                )
                                .unwrap()
                                .to_rfc2822(),
                                pair_address = pair.address().to_string(),
                                "skipped close trade execution: {:?}",
                                err
                            );
                        })
                        .map(|_| {
                            TradeRequest::close(
                                block_message.block_number,
                                block_message.block_timestamp,
                                pair,
                                open_trade_metadata.indexed_trade().clone(),
                                *open_trade_metadata.tx_hash(),
                            )
                        }),
                    Some(trade) => {
                        Err(eyre::eyre!("unactionable trade state: {:?}", trade.label()))
                    }
                };

                // Dispatch the trade request if strategy succeeds
                if let Ok(trade_request) = trade_request {
                    let trade_controller = self.trade_controller.clone();
                    let block_timestamp = block_message.block_timestamp;

                    pending_tx_tasks.spawn(async move {
                        debug!(
                            datetime = DateTime::from_timestamp(block_timestamp as i64, 0)
                                .unwrap()
                                .to_rfc2822(),
                            "executing trade request: {:?}", trade_request
                        );

                        let token_address = trade_request.pair.token_address().clone();
                        let op_label = trade_request.op.label();
                        let block_number = trade_request.block_number;

                        let res = match &trade_request.op {
                            TradeRequestOp::Open => {
                                trade_controller.open_position(trade_request).await
                            }
                            TradeRequestOp::Close { .. } => {
                                trade_controller.close_position(trade_request).await
                            }
                        };

                        match res {
                            Ok(_) => {
                                info!(
                                    block_number = block_number,
                                    token_address = token_address.to_string(),
                                    "executed {} trade request",
                                    op_label
                                );
                            }
                            Err(err) => {
                                error!(
                                    block_number = block_number,
                                    token_address = token_address.to_string(),
                                    "failed to execute {} trade request: {:?}",
                                    op_label,
                                    err
                                );
                            }
                        }
                    });
                }
            }
        }

        // Await completion of all pending tx submissions
        if !pending_tx_tasks.is_empty() {
            while let Some(pending_tx_result) = pending_tx_tasks.join_next().await {
                let _ = pending_tx_result.inspect_err(|e| {
                    error!("join set execution error: {:?}", e);
                });
            }
        }

        Ok(())
    }
}
