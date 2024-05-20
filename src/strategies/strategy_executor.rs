use super::Strategy;

use crate::{
    indexer::{ResolutionTimestamp, TimePriceBarStore},
    primitives::BlockMessage,
    trade_controller::{Trade, TradeController, TradeRequest, TradeRequestOp},
};

use alloy::{
    network::Ethereum,
    primitives::{address, Address},
    providers::Provider,
    transports::Transport,
};
use eyre::Result;
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, error, info, instrument};

const TARGET_PAIR_ADDRESS: Address = address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA");

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
    pub async fn on_indexed_block_message(
        &self,
        block_message: BlockMessage,
        time_price_bar_store: &TimePriceBarStore,
    ) -> Result<()> {
        let mut pending_tx_tasks = JoinSet::new();

        // Execute core strategy logic
        {
            let trades = self.trade_controller.trades().0.read().unwrap();
            let time_price_bars = time_price_bar_store.time_price_bars().read().unwrap();

            let resolution = time_price_bar_store.resolution();
            let resolution_timestamp =
                ResolutionTimestamp::from_timestamp(block_message.block_timestamp, &resolution);

            for pair in block_message.pairs.into_iter() {
                if cfg!(feature = "local") && *pair.address() != TARGET_PAIR_ADDRESS {
                    continue;
                }

                // note that time price bars are by pair, trades are by token
                let pair_time_price_bars =
                    time_price_bars.get(pair.address()).unwrap_or_else(|| {
                        panic!(
                            "missing time price bars for pair: {}",
                            pair.address().to_string()
                        )
                    });
                let trade_request = match trades
                    .get(pair.token_address())
                    .and_then(|address_trades| address_trades.active().as_ref())
                {
                    None => self
                        .strategy
                        .should_open_position(&pair_time_price_bars, &resolution_timestamp)
                        .inspect_err(|err| {
                            debug!(
                                block_number = block_message.block_number,
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
                            &resolution_timestamp,
                            &ResolutionTimestamp::from_timestamp(
                                *open_trade_metadata.block_timestamp(),
                                &resolution,
                            ),
                        )
                        .inspect_err(|err| {
                            debug!(
                                block_number = block_message.block_number,
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
                            )
                        }),
                    Some(trade) => {
                        Err(eyre::eyre!("unactionable trade state: {:?}", trade.label()))
                    }
                };

                // Dispatch the trade request if strategy succeeds
                if let Ok(trade_request) = trade_request {
                    let trade_controller = self.trade_controller.clone();

                    pending_tx_tasks.spawn(async move {
                        debug!("executing trade request: {:?}", trade_request);

                        let token_address = trade_request.pair.token_address().clone();
                        let op_label = trade_request.op.label();
                        let block_number = trade_request.block_number;

                        let res = match &trade_request.op {
                            TradeRequestOp::Open => {
                                trade_controller.open_position(trade_request).await
                            }
                            TradeRequestOp::Close(_) => {
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
