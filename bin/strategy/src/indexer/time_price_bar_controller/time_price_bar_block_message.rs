use pochtecatl_primitives::Pair;

use alloy::primitives::BlockNumber;

pub struct TimePriceBarBlockMessage {
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub updated_pairs: Vec<Pair>,
}

impl TimePriceBarBlockMessage {
    pub fn new(block_number: BlockNumber, block_timestamp: u64, updated_pairs: Vec<Pair>) -> Self {
        Self {
            block_number,
            block_timestamp,
            updated_pairs,
        }
    }
}
