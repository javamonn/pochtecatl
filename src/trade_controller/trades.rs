use crate::indexer::ParseableTrade;
use super::{Trade, TradeMetadata, AddressTrades};

use alloy::primitives::Address;

use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};
use eyre::{Result, eyre};

#[derive(Clone)]
pub struct Trades<P: ParseableTrade>(pub Arc<RwLock<FnvHashMap<Address, AddressTrades<P>>>>);
impl<P: ParseableTrade> Default for Trades<P> {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(FnvHashMap::default())))
    }
}

impl<P: ParseableTrade> Trades<P> {
    pub fn set_active(
        &self,
        address: &Address,
        trade: Option<Trade<P>>,
    ) -> Result<Option<Trade<P>>> {
        self.0
            .write()
            .unwrap()
            .get_mut(address)
            .map(|trades| trades.set_active(trade))
            .ok_or_else(|| eyre!("Missing trade for {}", address))
    }

    pub fn close(
        &self,
        address: &Address,
        open_trade: TradeMetadata<P>,
        committed_trade: TradeMetadata<P>,
    ) -> Result<()> {
        self.0
            .write()
            .unwrap()
            .get_mut(address)
            .map(|trades| trades.close(open_trade, committed_trade))
            .ok_or_else(|| eyre!("Missing trade for {}", address))
    }
}
