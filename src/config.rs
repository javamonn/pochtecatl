use eyre::Context;

use crate::primitives::BlockId;
use std::env;

pub fn rust_log() -> String {
    env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string())
}

pub fn start_block_id() -> BlockId {
    env::var("START_BLOCK_ID")
        .wrap_err("Failed to read START_BLOCK_ID from env")
        .and_then(|id| id.parse())
        .unwrap_or_else(|_| BlockId::Latest)
}

pub fn end_block_id() -> BlockId {
    env::var("END_BLOCK_ID")
        .wrap_err("Failed to read END_BLOCK_ID from env")
        .and_then(|id| id.parse())
        .unwrap_or_else(|_| BlockId::Latest)
}

pub fn rpc_url() -> String {
    env::var("RPC_URL")
        .wrap_err("Failed to read RPC_URL from env")
        .unwrap()
}
