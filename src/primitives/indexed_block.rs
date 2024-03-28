use alloy::primitives::{Address, BlockHash, BlockNumber, U128};
use fnv::FnvHashMap;

pub struct Pair {
    token_address: Address,
    token_reserve: U128,
    weth_reserve: U128,
}

pub struct IndexedBlock {
    block_number: BlockNumber,
    block_hash: BlockHash,
    pairs: FnvHashMap<Address, Pair>,
}
