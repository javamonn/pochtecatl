use crate::primitives::BlockId;

use alloy::primitives::{Address, FixedBytes};

use eyre::Context;
use lazy_static::lazy_static;
use std::{env, ffi::OsStr, sync::Once};

static DOTENV_INIT: Once = Once::new();

fn get_env_var<K: AsRef<OsStr>>(k: K) -> Result<String, env::VarError> {
    if cfg!(test) {
        DOTENV_INIT.call_once(|| {
            dotenvy::dotenv().expect(".env not found");
        });
    }

    env::var(k)
}

lazy_static! {
    pub static ref RUST_LOG: String =
        get_env_var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    pub static ref START_BLOCK_ID: BlockId = get_env_var("START_BLOCK_ID")
        .wrap_err("Failed to read START_BLOCK_ID from env")
        .and_then(|id| id.parse())
        .unwrap_or_else(|_| BlockId::Latest);
    pub static ref END_BLOCK_ID: BlockId = get_env_var("END_BLOCK_ID")
        .wrap_err("Failed to read END_BLOCK_ID from env")
        .and_then(|id| id.parse())
        .unwrap_or_else(|_| BlockId::Latest);
    pub static ref RPC_URL: String = get_env_var("RPC_URL")
        .wrap_err("Failed to read RPC_URL from env")
        .unwrap();
    pub static ref WETH_ADDRESS: Address = get_env_var("WETH_ADDRESS")
        .wrap_err("Failed to read WETH_ADDRESS from env")
        .and_then(|a| a.parse().wrap_err("Failed to parse WETH_ADDRESS"))
        .unwrap();
    pub static ref EXECUTOR_ADDRESS: Address = get_env_var("EXECUTOR_ADDRESS")
        .wrap_err("Failed to read EXECUTOR_ADDRESS from env")
        .and_then(|a| a.parse().wrap_err("Failed to parse EXECUTOR_ADDRESS"))
        .unwrap();
    pub static ref UNISWAP_V2_ROUTER_02_ADDRESS: Address =
        get_env_var("UNISWAP_V2_ROUTER_02_ADDRESS")
            .wrap_err("Failed to read UNISWAP_V2_ROUTER_02_ADDRESS from env")
            .and_then(|a| a
                .parse()
                .wrap_err("Failed to parse UNISWAP_V2_ROUTER_02_ADDRESS"))
            .unwrap();
    pub static ref WALLET_PRIVATE_KEY: FixedBytes<32> =
        get_env_var("WALLET_PRIVATE_KEY")
            .wrap_err("Failed to read WALLET_PRIVATE_KEY from env")
            .and_then(|key| hex::decode(&key).wrap_err("Failed to decode WALLET_PRIVATE_KEY"))
            .and_then(
                |key| FixedBytes::try_from(key.as_slice()).wrap_err("Failed to create FixedBytes")
            )
            .unwrap();
    pub static ref IS_BACKFILL: bool = match (END_BLOCK_ID.deref(), START_BLOCK_ID.deref()) {
        (BlockId::Latest, BlockId::Latest) => false,
        _ => true,
    };
}
