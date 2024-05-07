use super::{StrategyExecutor, UniswapV2MomentumStrategy, UniswapV2Strategy};

use crate::{
    config,
    indexer::{IndexedBlockMessage, ResolutionTimestamp, TimePriceBarStore, UniswapV2PairTrade},
    trade_controller::{
        Trade, TradeController, UniswapV2CloseTradeRequest, UniswapV2OpenTradeRequest,
    },
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

const TARGET_PAIR_ADDRESS: Address = address!("377FeeeD4820B3B28D1ab429509e7A0789824fCA");

pub struct UniswapV2StrategyExecuctor<S, T, P>
where
    S: UniswapV2Strategy,
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    strategy: Arc<S>,
    trade_controller: Arc<TradeController<UniswapV2PairTrade, T, P>>,
}

impl<S, T, P> UniswapV2StrategyExecuctor<S, T, P>
where
    S: UniswapV2Strategy,
    T: Transport + Clone,
    P: Provider<T, Ethereum>,
{
    pub fn new(
        trade_controller: Arc<TradeController<UniswapV2PairTrade, T, P>>,
        strategy: S,
    ) -> Self {
        Self {
            trade_controller,
            strategy: Arc::new(strategy),
        }
    }

    pub fn with_momentum_strategy(
        trade_controller: Arc<TradeController<UniswapV2PairTrade, T, P>>,
    ) -> UniswapV2StrategyExecuctor<UniswapV2MomentumStrategy, T, P> {
        UniswapV2StrategyExecuctor::new(trade_controller, UniswapV2MomentumStrategy::new())
    }
}

impl<S, T, P> StrategyExecutor for UniswapV2StrategyExecuctor<S, T, P>
where
    S: UniswapV2Strategy,
    T: Transport + Clone,
    P: Provider<T, Ethereum>,
{
    #[instrument(skip_all, fields(block_number=indexed_block_message.block_number))]
    async fn on_indexed_block_message(
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
            let timestamp = ResolutionTimestamp::from_timestamp(
                indexed_block_message.block_timestamp,
                &resolution,
            );

            for uniswap_v2_pair in indexed_block_message.uniswap_v2_pairs.into_iter() {
                if cfg!(feature = "local") && uniswap_v2_pair.pair_address != TARGET_PAIR_ADDRESS {
                    continue;
                }

                match trades
                    .get(&uniswap_v2_pair.pair_address)
                    .and_then(|address_trades| address_trades.active().as_ref())
                {
                    None => {
                        match self.strategy.should_open_position(
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
                                let trade_controller = Arc::clone(&self.trade_controller);

                                pending_tx_tasks.spawn(async move {
                                    let _ =
                                        trade_controller.open_position(open_position_request).await;

                                    info!(
                                        block_number = indexed_block_message.block_number,
                                        pair_address = uniswap_v2_pair.pair_address.to_string(),
                                        "executed opened position"
                                    );
                                });
                            }
                            Err(err) => {
                                debug!(
                                    block_number = indexed_block_message.block_number,
                                    pair_address = uniswap_v2_pair.pair_address.to_string(),
                                    "Skipping open position: {:?}",
                                    err
                                );
                            }
                        }
                    }
                    Some(Trade::Open(open_trade_metadata)) => {
                        match self.strategy.should_close_position(
                            &uniswap_v2_pair,
                            &timestamp,
                            &resolution,
                            &open_trade_metadata,
                            &time_price_bars,
                        ) {
                            Ok(()) => {
                                let open_token_amount =
                                    if uniswap_v2_pair.token_address < *config::WETH_ADDRESS {
                                        // token0 is token, token1 is weth
                                        open_trade_metadata.parsed_trade().amount0_out
                                    } else {
                                        // token1 is token, token0 is weth
                                        open_trade_metadata.parsed_trade().amount1_out
                                    };

                                let close_position_request = UniswapV2CloseTradeRequest::new(
                                    uniswap_v2_pair.pair_address,
                                    uniswap_v2_pair.token_address,
                                    uniswap_v2_pair.weth_reserve,
                                    uniswap_v2_pair.token_reserve,
                                    open_token_amount,
                                    indexed_block_message.block_number,
                                    indexed_block_message.block_timestamp,
                                );

                                let trade_controller = Arc::clone(&self.trade_controller);
                                pending_tx_tasks.spawn(async move {
                                    let _ = trade_controller
                                        .close_position(close_position_request)
                                        .await;

                                    info!(
                                        block_number = indexed_block_message.block_number,
                                        pair_address = uniswap_v2_pair.pair_address.to_string(),
                                        "executed close position"
                                    );
                                });
                            }
                            Err(err) => {
                                debug!(
                                    block_number = indexed_block_message.block_number,
                                    pair_address = uniswap_v2_pair.pair_address.to_string(),
                                    "Skipping close position: {:?}",
                                    err
                                );
                            }
                        }
                    }
                    _ => { /* noop */ }
                };
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
