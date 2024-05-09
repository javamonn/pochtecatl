use super::{uniswap_v2_dex_provider, Strategy};

use crate::{
    indexer::{IndexedBlockMessage, ResolutionTimestamp, TimePriceBarStore},
    trade_controller::{Trade, TradeController, TradeControllerRequest, TradeRequestIntent},
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
use tracing::{debug, error};

const TARGET_PAIR_ADDRESS: Address = address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA");

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

    pub async fn on_indexed_block_message(
        &self,
        indexed_block_message: IndexedBlockMessage,
        time_price_bar_store: &TimePriceBarStore,
    ) -> Result<()> {
        let mut pending_tx_tasks = JoinSet::new();

        // Execute core strategy logic
        {
            let trades = self.trade_controller.trades().0.read().unwrap();
            let time_price_bars = time_price_bar_store.time_price_bars().read().unwrap();

            let resolution = time_price_bar_store.resolution();
            let resolution_timestamp = ResolutionTimestamp::from_timestamp(
                indexed_block_message.block_timestamp,
                &resolution,
            );

            for pair in indexed_block_message.uniswap_v2_pairs.into_iter() {
                if cfg!(feature = "local") && pair.pair_address != TARGET_PAIR_ADDRESS {
                    continue;
                }

                let pair_time_price_bars = time_price_bars.get(&pair.pair_address).unwrap();
                let trade_request = match trades
                    .get(&pair.pair_address)
                    .and_then(|address_trades| address_trades.active().as_ref())
                {
                    None => self
                        .strategy
                        .should_open_position(&pair_time_price_bars, &resolution_timestamp)
                        .map(|_| {
                            uniswap_v2_dex_provider::make_open_trade_request(
                                &pair,
                                indexed_block_message.block_number,
                                indexed_block_message.block_timestamp,
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
                        .and_then(|_| {
                            uniswap_v2_dex_provider::make_close_trade_request(
                                open_trade_metadata,
                                &pair,
                                indexed_block_message.block_number,
                                indexed_block_message.block_timestamp,
                            )
                        }),
                    Some(trade) => {
                        Err(eyre::eyre!("unactionable trade state: {:?}", trade.label()))
                    }
                };

                // Dispatch the trade request if strategy succeeds
                match trade_request {
                    Ok(trade_request) => {
                        let trade_controller = self.trade_controller.clone();

                        pending_tx_tasks.spawn(async move {
                            debug!(
                                block_number = indexed_block_message.block_number,
                                pair_address = pair.pair_address.to_string(),
                                "executing trade: {:?}",
                                trade_request
                            );

                            let _ = match trade_request.intent() {
                                TradeRequestIntent::Open { .. } => {
                                    trade_controller.open_position(trade_request).await
                                }
                                TradeRequestIntent::Close { .. } => {
                                    trade_controller.close_position(trade_request).await
                                }
                            };
                        });
                    }
                    Err(err) => {
                        debug!(
                            block_number = indexed_block_message.block_number,
                            pair_address = pair.pair_address.to_string(),
                            "skipped trade execution: {:?}",
                            err
                        );
                    }
                }
            }
        }

        // Await completion of all pending tx submissions
        if !pending_tx_tasks.is_empty() {
            while let Some(pending_tx_result) = pending_tx_tasks.join_next().await {
                let _ = pending_tx_result.inspect_err(|err| {
                    error!(
                        block_number = indexed_block_message.block_number,
                        "join set execution error: {:?}", err
                    );
                });
            }
        }

        Ok(())
    }
}
