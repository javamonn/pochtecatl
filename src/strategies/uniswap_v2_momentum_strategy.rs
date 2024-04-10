use crate::{
    abi::uniswap_v2_router,
    indexer::{IndexedBlockMessage, IndexedUniswapV2Pair, TimePriceBarStore},
    trade_position_controller::{CommittedTrade, TradePosition, TradePositionController},
};

use alloy::{
    primitives::{uint, Address, U256},
    rpc::types::eth::TransactionRequest,
};

use eyre::{eyre, Result};
use std::sync::{mpsc::Receiver, Arc};
use tokio::task::{JoinHandle, JoinSet};

const BP_FACTOR: U256 = uint!(10000_U256);
const MAX_TRADE_SIZE_PRICE_IMPACT_BP: U256 = uint!(50_U256);
const MAX_TRADE_SIZE_WEI: U256 = uint!(1000000000000000000_U256);

pub struct UniswapV2MomentumStrategy {
    exec_handle: Option<JoinHandle<Result<()>>>,
    time_price_bar_store: Arc<TimePriceBarStore>,
    trade_position_controller: Arc<TradePositionController>,
    signer_address: Address,
}

fn should_open_position(
    time_price_bar_store: &TimePriceBarStore,
    uniswap_v2_pair: &IndexedUniswapV2Pair,
) -> bool {
    unimplemented!();
}

fn should_close_position(
    time_price_bar_store: &TimePriceBarStore,
    uniswap_v2_pair: &IndexedUniswapV2Pair,
    committed_trade: &CommittedTrade,
) -> bool {
    unimplemented!();
}

fn make_open_position_tx_request(
    pair: &IndexedUniswapV2Pair,
    signer_address: Address,
    block_timestamp: u64,
) -> TransactionRequest {
    let eth_amount_in = {
        let max_for_price_impact = (MAX_TRADE_SIZE_PRICE_IMPACT_BP * pair.weth_reserve) / BP_FACTOR;
        if max_for_price_impact < MAX_TRADE_SIZE_WEI {
            max_for_price_impact
        } else {
            MAX_TRADE_SIZE_WEI
        }
    };

    let output_token_amount_min =
        uniswap_v2_router::get_amount_out(eth_amount_in, pair.weth_reserve, pair.token_reserve);

    uniswap_v2_router::swap_exact_eth_for_tokens_tx_request(
        signer_address,
        eth_amount_in,
        output_token_amount_min,
        pair.token_address,
        U256::from(block_timestamp + 30),
    )
}

fn make_close_position_tx_request(
    pair: &IndexedUniswapV2Pair,
    committed_open_trade: &CommittedTrade,
    signer_address: Address,
    block_timestamp: u64,
) -> TransactionRequest {
    unimplemented!()
}

async fn handle_indexed_block_message(
    indexed_block_message: IndexedBlockMessage,
    signer_address: Address,
    time_price_bar_store: Arc<TimePriceBarStore>,
    trade_position_controller: Arc<TradePositionController>,
) -> Result<()> {
    let mut tasks = JoinSet::new();

    let trade_positions = trade_position_controller.trade_positions().read().unwrap();
    for uniswap_v2_pair in indexed_block_message.uniswap_v2_pairs.into_iter() {
        let position = trade_positions.get(&uniswap_v2_pair.pair_address);

        match position {
            None if should_open_position(&time_price_bar_store, &uniswap_v2_pair) => {
                let tx_request = make_open_position_tx_request(
                    &uniswap_v2_pair,
                    signer_address,
                    indexed_block_message.block_timestamp,
                );
                let pair_address = uniswap_v2_pair.pair_address;

                let trade_position_controller = Arc::clone(&trade_position_controller);

                tasks.spawn(async move {
                    match trade_position_controller
                        .uniswap_v2_position_is_valid(&uniswap_v2_pair, &tx_request)
                        .await
                    {
                        Ok(true) => {
                            trade_position_controller
                                .open_position(tx_request, pair_address)
                                .await
                        }
                        Ok(false) => {
                            log::warn!(
                                "Uniswap V2 position is invalid for pair {}",
                                pair_address
                            ); 

                            // Percolate OK as this is not an execution error
                            Ok(())
                        }
                        Err(err) => {
                            Err(err)
                        }
                    }
                });
            }
            Some(TradePosition::Open(committed_open_trade))
                if should_close_position(
                    &time_price_bar_store,
                    &uniswap_v2_pair,
                    &committed_open_trade,
                ) =>
            {
                let tx_request = make_close_position_tx_request(
                    &uniswap_v2_pair,
                    &committed_open_trade,
                    signer_address,
                    indexed_block_message.block_timestamp,
                );
                let pair_address = uniswap_v2_pair.pair_address;

                let trade_position_controller = Arc::clone(&trade_position_controller);
                tasks.spawn(async move {
                    trade_position_controller
                        .close_position(tx_request, pair_address)
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
        signer_address: Address,
        time_price_bar_store: Arc<TimePriceBarStore>,
        trade_position_controller: Arc<TradePositionController>,
    ) -> Self {
        Self {
            exec_handle: None,
            time_price_bar_store,
            trade_position_controller,
            signer_address,
        }
    }

    pub fn exec(&mut self, indexed_block_message_receiver: Receiver<IndexedBlockMessage>) {
        let time_price_bar_store = Arc::clone(&self.time_price_bar_store);
        let trade_position_controller = Arc::clone(&self.trade_position_controller);
        let signer_address = self.signer_address;

        let exec_handle = tokio::spawn(async move {
            while let Ok(indexed_block_message) = indexed_block_message_receiver.recv() {
                let time_price_bar_store = Arc::clone(&time_price_bar_store);
                let trade_position_controller = Arc::clone(&trade_position_controller);

                handle_indexed_block_message(
                    indexed_block_message,
                    signer_address,
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
