use alloy::primitives::{address, uint, Address, U256};

pub const MAX_TRADE_SIZE_WEI: U256 = uint!(1000000000000000000_U256);

// TODO: These are chain dependent, but assume Base for now
pub const AVERAGE_BLOCK_TIME_SECONDS: u64 = 2;
pub const WETH_ADDRESS: Address = address!("4200000000000000000000000000000000000006");
pub const UNISWAP_V2_ROUTER_02_ADDRESS: Address =
    address!("4752ba5dbc23f44d87826276bf6fd6b1c372ad24");
pub const UNISWAP_V3_QUOTER_V2_ADDRESS: Address =
    address!("3d4e44Eb1374240CE5F1B871ab261CD16335B76a");
pub const UNISWAP_V3_ROUTER_02_ADDRESS: Address =
    address!("2626664c2603336E57B271c5C0b26F421741e481");
pub const UNISWAP_V3_FACTORY_ADDRESS: Address =
    address!("33128a8fC17869897dcE68Ed026d694621f6FDfD");
pub const MULTICALL3_ADDRESS: Address = address!("ca11bde05977b3631167028862be2a173976ca11");
