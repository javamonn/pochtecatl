use crate::primitives::BlockId;

use alloy::primitives::Address;

use eyre::Context;
use lazy_static::lazy_static;
use std::{env, ops::Deref};

lazy_static! {
    pub static ref RUST_LOG: String = env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    pub static ref START_BLOCK_ID: BlockId = env::var("START_BLOCK_ID")
        .wrap_err("Failed to read START_BLOCK_ID from env")
        .and_then(|id| id.parse())
        .unwrap_or_else(|_| BlockId::Latest);
    pub static ref END_BLOCK_ID: BlockId = env::var("END_BLOCK_ID")
        .wrap_err("Failed to read END_BLOCK_ID from env")
        .and_then(|id| id.parse())
        .unwrap_or_else(|_| BlockId::Latest);
    pub static ref RPC_URL: String = env::var("RPC_URL")
        .wrap_err("Failed to read RPC_URL from env")
        .unwrap();
    pub static ref WETH_ADDRESS: Address = env::var("WETH_ADDRESS")
        .wrap_err("Failed to read WETH_ADDRESS from env")
        .and_then(|a| a.parse().wrap_err("Failed to parse WETH_ADDRESS"))
        .unwrap();
    pub static ref IS_BACKFILL: bool = match (END_BLOCK_ID.deref(), START_BLOCK_ID.deref()) {
        (BlockId::Latest, BlockId::Latest) => false,
        _ => true,
    };
}
