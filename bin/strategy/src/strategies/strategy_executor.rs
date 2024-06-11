use super::Strategy;

use crate::{
    indexer::TimePriceBarStore,
    trade_controller::{Trade, TradeController, TradeRequest},
};

use pochtecatl_primitives::{BlockMessage, TradeRequestOp};

use alloy::{
    network::Ethereum,
    primitives::{address, Address},
    providers::Provider,
    transports::Transport,
};
use chrono::DateTime;
use eyre::Result;
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, error, info, instrument};

lazy_static! {
    static ref TARGET_PAIR_ADDRESSES: Vec<Address> = vec![
        // degen
        address!("c9034c3E7F58003E6ae0C8438e7c8f4598d5ACAA"),
        // toshi
        address!("4b0Aaf3EBb163dd45F663b38b6d93f6093EBC2d3"),
        // brett
        address!("BA3F945812a83471d709BCe9C3CA699A19FB46f7"),
        // mfer
        address!("7EC18ABf80E865c6799069df91073335935C4185")
    ];
}

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
            for pair in block_message.pairs.into_iter() {
                if cfg!(feature = "local") && !TARGET_PAIR_ADDRESSES.contains(&pair.address()) {
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
