use crate::{IndexedTrade, Pair, PairBlockTick};

use pochtecatl_db::BlockModel;

use alloy::primitives::{Address, BlockHash, BlockNumber};

use fnv::FnvHashMap;
use serde::Serialize;
use tracing::error;

#[derive(Serialize, Debug)]
pub struct Block {
    pub block_hash: Option<BlockHash>,
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub pair_ticks: FnvHashMap<Address, PairBlockTick>,
}

impl Block {
    pub fn new(
        block_hash: Option<BlockHash>,
        block_number: BlockNumber,
        block_timestamp: u64,
    ) -> Self {
        Self {
            block_hash,
            block_number,
            block_timestamp,
            pair_ticks: FnvHashMap::default(),
        }
    }

    pub fn add_trade(&mut self, trade: IndexedTrade, pair: &Pair) {
        self.pair_ticks
            .entry(*trade.pair_address())
            .and_modify(|pair_block_tick| {
                if let Err(e) = pair_block_tick.add_indexed_trade(&trade) {
                    error!("Error adding trade: {:?}", e)
                };
            })
            .or_insert_with(|| PairBlockTick::new(trade, pair.clone()).unwrap());
    }
}

impl From<BlockModel> for Block {
    fn from(value: BlockModel) -> Self {
        Self {
            block_hash: None,
            block_number: value.number.into(),
            block_timestamp: value.timestamp.into(),
            pair_ticks: serde_json::from_value(value.pair_ticks).unwrap(),
        }
    }
}

impl From<Block> for BlockModel {
    fn from(value: Block) -> BlockModel {
        BlockModel {
            number: value.block_number.into(),
            timestamp: value.block_timestamp.into(),
            pair_ticks: serde_json::to_value(value.pair_ticks).unwrap(),
        }
    }
}

impl From<&Block> for BlockModel {
    fn from(value: &Block) -> Self {
        Self {
            number: value.block_number.into(),
            timestamp: value.block_timestamp.into(),
            pair_ticks: serde_json::to_value(value.pair_ticks.clone()).unwrap(),
        }
    }
}

