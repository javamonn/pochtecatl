use crate::primitives::BlockId;

use eyre::eyre;
use alloy_primitives::BlockNumber;

pub enum IndexTarget {
    Latest,
    Range {
        start: BlockNumber,
        end: BlockNumber,
    },
}

impl IndexTarget {
    pub fn from_block_ids(
        start_block_id: BlockId,
        end_block_id: BlockId,
    ) -> eyre::Result<IndexTarget> {
        match (start_block_id, end_block_id) {
            (BlockId::BlockNumber(start), BlockId::BlockNumber(end)) => {
                if start < end {
                    Ok(IndexTarget::Range { start, end })
                } else {
                    Err(eyre!("Failed to create IndexTarget due to invalid block numbers: start {}, end {}", start, end))
                }
            }
            (BlockId::Latest, BlockId::Latest) => Ok(IndexTarget::Latest),
            _ => Err(eyre!("Failed to create IndexTarget")),
        }
    }
}

