use super::{Block, TimePriceBarStore};

use crate::rpc_provider::RpcProvider;

use alloy::primitives::{Address, BlockNumber, U256};
use std::sync::{mpsc::Receiver, Arc};
use tokio::sync::oneshot;

pub struct IndexedUniswapV2Pair {
    pub reserve0: U256,
    pub reserve1: U256,
    pub token_address: Address,
    pub pair_address: Address,
}

impl IndexedUniswapV2Pair {
    pub fn new(
        reserve0: U256,
        reserve1: U256,
        token_address: Address,
        pair_address: Address,
    ) -> Self {
        Self {
            reserve0,
            reserve1,
            token_address,
            pair_address,
        }
    }
}

pub struct IndexedBlockMessage {
    pub block_number: BlockNumber,
    pub uniswap_v2_pairs: Vec<IndexedUniswapV2Pair>,
    pub ack: Option<oneshot::Sender<()>>,
}

impl IndexedBlockMessage {
    pub fn new(
        block_number: BlockNumber,
        uniswap_v2_pairs: Vec<IndexedUniswapV2Pair>,
        ack: Option<oneshot::Sender<()>>,
    ) -> Self {
        Self {
            block_number,
            uniswap_v2_pairs,
            ack,
        }
    }

    pub fn from_block_with_ack(block: &Block) -> (Self, oneshot::Receiver<()>) {
        let (ack_sender, ack_receiver) = oneshot::channel();
        let inst = Self {
            block_number: block.block_number,
            uniswap_v2_pairs: block
                .uniswap_v2_pairs
                .iter()
                .filter_map(|(pair_address, pair)| match pair.trades.last() {
                    Some(trade) => Some(IndexedUniswapV2Pair::new(
                        trade.reserve0,
                        trade.reserve1,
                        pair.token_address,
                        *pair_address,
                    )),
                    None => None,
                })
                .collect(),
            ack: Some(ack_sender),
        };

        (inst, ack_receiver)
    }
}

pub trait Indexer {
    fn subscribe(&mut self, rpc_provider: &Arc<RpcProvider>) -> Receiver<IndexedBlockMessage>;
    fn time_price_bar_store(&self) -> Arc<TimePriceBarStore>;
}
