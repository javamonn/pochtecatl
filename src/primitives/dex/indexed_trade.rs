use super::{UniswapV2IndexedTrade, UniswapV3IndexedTrade};

use alloy::{
    primitives::{Address, FixedBytes},
    rpc::types::eth::{Log, TransactionReceipt},
};

use fraction::GenericFraction;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};

pub trait DexIndexedTrade {
    // get the token price before the trade
    fn token_price_before(&self, token_address: &Address) -> GenericFraction<BigUint>;
    // get the token price after the trade
    fn token_price_after(&self, token_address: &Address) -> GenericFraction<BigUint>;
    // get the total weth volume of the trade
    fn weth_volume(&self, token_address: &Address) -> BigUint;
    // get the pair address
    fn pair_address(&self) -> &Address;
    // get the event signature hashes required for parsing the trade
    fn event_signature_hashes() -> Vec<FixedBytes<32>>;
}

pub struct IndexedTradeParseContext<'l> {
    idx: usize,
    logs: &'l Vec<Log>,
}

impl<'l> From<&'l Vec<Log>> for IndexedTradeParseContext<'l> {
    fn from(logs: &'l Vec<Log>) -> Self {
        Self { idx: 0, logs }
    }
}

impl<'l> IndexedTradeParseContext<'l> {
    pub fn new(idx: usize, logs: &'l Vec<Log>) -> Self {
        Self { idx, logs }
    }

    pub fn logs(&self) -> &Vec<Log> {
        &self.logs
    }

    pub fn idx(&self) -> usize {
        self.idx
    }

    pub fn next(&mut self) {
        self.idx += 1;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexedTrade {
    UniswapV2(UniswapV2IndexedTrade),
    UniswapV3(UniswapV3IndexedTrade),
}

impl IndexedTrade {
    pub fn weth_volume(&self, token_address: &Address) -> BigUint {
        match self {
            IndexedTrade::UniswapV2(trade) => trade.weth_volume(token_address),
            IndexedTrade::UniswapV3(trade) => trade.weth_volume(token_address),
        }
    }

    pub fn token_price_before(&self, token_address: &Address) -> GenericFraction<BigUint> {
        match self {
            IndexedTrade::UniswapV2(trade) => trade.token_price_before(token_address),
            IndexedTrade::UniswapV3(trade) => trade.token_price_before(token_address),
        }
    }

    pub fn token_price_after(&self, token_address: &Address) -> GenericFraction<BigUint> {
        match self {
            IndexedTrade::UniswapV2(trade) => trade.token_price_after(token_address),
            IndexedTrade::UniswapV3(trade) => trade.token_price_after(token_address),
        }
    }

    pub fn pair_address(&self) -> &Address {
        match self {
            IndexedTrade::UniswapV2(trade) => trade.pair_address(),
            IndexedTrade::UniswapV3(trade) => trade.pair_address(),
        }
    }

    pub fn from_logs(logs: &Vec<Log>) -> Vec<Self> {
        let mut trades = Vec::new();
        let mut ctx = IndexedTradeParseContext::from(logs);

        while ctx.idx() < ctx.logs().len() {
            if let Ok(trade) = Self::try_from(&ctx) {
                trades.push(trade);
            }
            ctx.next();
        }

        trades
    }

    pub fn from_receipt(receipt: &TransactionReceipt) -> Vec<Self> {
        receipt
            .as_ref()
            .as_receipt()
            .map(|r| Self::from_logs(&r.logs))
            .unwrap_or_default()
    }

    pub fn event_signature_hashes() -> Vec<FixedBytes<32>> {
        vec![
            UniswapV2IndexedTrade::event_signature_hashes(),
            UniswapV3IndexedTrade::event_signature_hashes(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

impl TryFrom<&IndexedTradeParseContext<'_>> for IndexedTrade {
    type Error = eyre::Report;

    fn try_from(value: &IndexedTradeParseContext) -> Result<Self, Self::Error> {
        if let Ok(indexed_trade) = UniswapV2IndexedTrade::try_from(value) {
            Ok(IndexedTrade::UniswapV2(indexed_trade))
        } else if let Ok(indexed_trade) = UniswapV3IndexedTrade::try_from(value) {
            Ok(IndexedTrade::UniswapV3(indexed_trade))
        } else {
            Err(eyre::eyre!("Could not parse indexed trade"))
        }
    }
}

impl From<UniswapV2IndexedTrade> for IndexedTrade {
    fn from(trade: UniswapV2IndexedTrade) -> Self {
        IndexedTrade::UniswapV2(trade)
    }
}

impl From<UniswapV3IndexedTrade> for IndexedTrade {
    fn from(trade: UniswapV3IndexedTrade) -> Self {
        IndexedTrade::UniswapV3(trade)
    }
}
