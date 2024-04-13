use crate::{
    indexer::{
        time_price_bar_indicators, IndexedBlockMessage, IndexedUniswapV2Pair, ResolutionTimestamp,
        TimePriceBarStore,
    },
    trade_position_controller::{
        ClosePositionRequest, CommittedTrade, OpenPositionRequest, TradePosition,
        TradePositionController,
    },
};

use eyre::{eyre, Context, Result};
use std::sync::{mpsc::Receiver, Arc};
use tokio::task::{JoinHandle, JoinSet};

pub struct UniswapV2MomentumStrategy {
    exec_handle: Option<JoinHandle<Result<()>>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
    trade_position_controller: Arc<TradePositionController>,
}

// The number of time bar resolutions to consider
pub const DATA_RANGE_SIZE: u64 = 24;

fn should_open_position(
    uniswap_v2_pair: &IndexedUniswapV2Pair,
    block_timestamp: u64,
    time_price_bar_store: &TimePriceBarStore,
) -> Result<()> {
    let time_price_bars = time_price_bar_store.time_price_bars().read().unwrap();
    let pair_time_price_bars = time_price_bars
        .get(&uniswap_v2_pair.pair_address)
        .ok_or_else(|| {
            eyre!(
                "No time price bars for pair: {:?}",
                uniswap_v2_pair.pair_address
            )
        })?;

    let end_resolution_timestamp =
        ResolutionTimestamp::from_timestamp(block_timestamp, time_price_bar_store.resolution());
    let start_resolution_timestamp =
        end_resolution_timestamp.decrement(time_price_bar_store.resolution(), DATA_RANGE_SIZE);

    // Ensure the most recent time price bar is positive
    if pair_time_price_bars
        .time_price_bar(&end_resolution_timestamp)
        .map(|time_price_bar| time_price_bar.is_negative())
        .unwrap_or(true)
    {
        return Err(eyre!("Last time price bar is negative"));
    }

    let indicators = pair_time_price_bars
        .indicators_range(&start_resolution_timestamp, &end_resolution_timestamp);

    Ok(())
}

fn should_close_position(
    time_price_bar_store: &TimePriceBarStore,
    uniswap_v2_pair: &IndexedUniswapV2Pair,
    committed_trade: &CommittedTrade,
) -> bool {
    unimplemented!();
}

async fn handle_indexed_block_message(
    indexed_block_message: IndexedBlockMessage,
    time_price_bar_store: Arc<TimePriceBarStore>,
    trade_position_controller: Arc<TradePositionController>,
) -> Result<()> {
    let mut tasks = JoinSet::new();

    let trade_positions = trade_position_controller.trade_positions().read().unwrap();
    for uniswap_v2_pair in indexed_block_message.uniswap_v2_pairs.into_iter() {
        let position = trade_positions.get(&uniswap_v2_pair.pair_address);

        match position {
            None if should_open_position(
                &uniswap_v2_pair,
                indexed_block_message.block_timestamp,
                &time_price_bar_store,
            )
            .is_ok() =>
            {
                let open_position_request = OpenPositionRequest::new(
                    uniswap_v2_pair.pair_address,
                    uniswap_v2_pair.token_address,
                    uniswap_v2_pair.weth_reserve,
                    uniswap_v2_pair.token_reserve,
                    indexed_block_message.block_number,
                    indexed_block_message.block_timestamp,
                );
                let trade_position_controller = Arc::clone(&trade_position_controller);

                tasks.spawn(async move {
                    trade_position_controller
                        .open_position(open_position_request)
                        .await
                });
            }
            Some(TradePosition::Open(committed_open_trade))
                if should_close_position(
                    &time_price_bar_store,
                    &uniswap_v2_pair,
                    &committed_open_trade,
                ) =>
            {
                let close_position_request = ClosePositionRequest::new();
                let trade_position_controller = Arc::clone(&trade_position_controller);

                tasks.spawn(async move {
                    trade_position_controller
                        .close_position(close_position_request)
                        .await
                });
            }
            _ => { /* noop */ }
        };
    }

    // If message included an ack, trigger it
    if let Some(ack) = indexed_block_message.ack {
        ack.send(())
            .map_err(|e| eyre!("Failed to send ack: {:?}", e))?;
    }

    Ok(())
}

impl UniswapV2MomentumStrategy {
    pub fn new(
        time_price_bar_store: Arc<TimePriceBarStore>,
        trade_position_controller: Arc<TradePositionController>,
    ) -> Self {
        Self {
            exec_handle: None,
            time_price_bar_store,
            trade_position_controller,
        }
    }

    pub fn exec(&mut self, indexed_block_message_receiver: Receiver<IndexedBlockMessage>) {
        let time_price_bar_store = Arc::clone(&self.time_price_bar_store);
        let trade_position_controller = Arc::clone(&self.trade_position_controller);

        let exec_handle = tokio::spawn(async move {
            while let Ok(indexed_block_message) = indexed_block_message_receiver.recv() {
                let time_price_bar_store = Arc::clone(&time_price_bar_store);
                let trade_position_controller = Arc::clone(&trade_position_controller);

                handle_indexed_block_message(
                    indexed_block_message,
                    time_price_bar_store,
                    trade_position_controller,
                )
                .await?;
            }

            Ok(())
        });

        self.exec_handle = Some(exec_handle);
    }

    pub async fn join(self) -> Result<()> {
        if let Some(exec_handle) = self.exec_handle {
            exec_handle.await??;
        }

        Ok(())
    }
}
