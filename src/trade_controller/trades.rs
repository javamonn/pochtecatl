use super::TradeMetadata;

use alloy::primitives::Address;

use eyre::{eyre, Result};
use fnv::FnvHashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug)]
pub enum Trade {
    PendingOpen,
    Open(TradeMetadata),
    PendingClose,
}

impl Trade {
    pub fn label(&self) -> &str {
        match self {
            Trade::PendingOpen => "Pending Open",
            Trade::Open(_) => "Open",
            Trade::PendingClose => "Pending Close",
        }
    }
}

pub struct AddressTrades {
    active: Option<Trade>,
    closed: Vec<(TradeMetadata, TradeMetadata)>,
}
impl AddressTrades {
    pub fn close(&mut self, open_trade: TradeMetadata, committed_trade: TradeMetadata) {
        self.active = None;
        self.closed.push((open_trade, committed_trade));
    }

    pub fn set_active(&mut self, trade: Option<Trade>) -> Option<Trade> {
        std::mem::replace(&mut self.active, trade)
    }

    pub fn active(&self) -> &Option<Trade> {
        &self.active
    }

    pub fn closed(&self) -> &Vec<(TradeMetadata, TradeMetadata)> {
        &self.closed
    }
}
impl Default for AddressTrades {
    fn default() -> Self {
        Self {
            active: None,
            closed: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct Trades(pub Arc<RwLock<FnvHashMap<Address, AddressTrades>>>);
impl Default for Trades {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(FnvHashMap::default())))
    }
}

impl Trades {
    pub fn set_active(&self, address: &Address, trade: Option<Trade>) -> Result<Option<Trade>> {
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
        open_trade: TradeMetadata,
        committed_trade: TradeMetadata,
    ) -> Result<()> {
        self.0
            .write()
            .unwrap()
            .get_mut(address)
            .map(|trades| trades.close(open_trade, committed_trade))
            .ok_or_else(|| eyre!("Missing trade for {}", address))
    }
}
