use super::{BlockBuilder, UniswapV2PairTrade};

use crate::{config, providers::RpcProvider};

use alloy::{
    network::Ethereum,
    primitives::{Address, BlockHash, BlockNumber},
    providers::Provider,
    rpc::types::eth::Log,
    transports::Transport,
};

use eyre::Result;
use fnv::FnvHashMap;
use std::sync::Arc;

pub struct Block {
    pub block_hash: Option<BlockHash>,
    pub block_number: BlockNumber,
    pub block_timestamp: u64,
    pub uniswap_v2_pairs: FnvHashMap<Address, UniswapV2Pair>,
}

