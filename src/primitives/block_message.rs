use super::{Block, Pair};

use alloy::primitives::BlockNumber;

pub struct BlockMessage {
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub pairs: Vec<Pair>,
}

impl BlockMessage {
    pub fn new(block_number: BlockNumber, block_timestamp: u64, pairs: Vec<Pair>) -> Self {
        Self {
            block_number,
            block_timestamp,
            pairs,
        }
    }
}

impl From<Block> for BlockMessage {
    fn from(value: Block) -> Self {
        Self::new(
            value.block_number,
            value.block_timestamp,
            value
                .pair_ticks
                .into_iter()
                .map(|(pair_address, pair_block_tick)| pair_block_tick.pair(pair_address))
                .collect(),
        )
    }
}
