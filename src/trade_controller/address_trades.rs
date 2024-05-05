use crate::indexer::ParseableTrade;
use super::{Trade, TradeMetadata};

pub struct AddressTrades<P: ParseableTrade> {
    active: Option<Trade<P>>,
    closed: Vec<(TradeMetadata<P>, TradeMetadata<P>)>,
}

impl<P: ParseableTrade> AddressTrades<P> {
    pub fn close(&mut self, open_trade: TradeMetadata<P>, committed_trade: TradeMetadata<P>) {
        self.active = None;
        self.closed.push((open_trade, committed_trade));
    }

    pub fn set_active(&mut self, trade: Option<Trade<P>>) -> Option<Trade<P>> {
        std::mem::replace(&mut self.active, trade)
    }

    pub fn active(&self) -> &Option<Trade<P>> {
        &self.active
    }

    pub fn closed(&self) -> &Vec<(TradeMetadata<P>, TradeMetadata<P>)> {
        &self.closed
    }
}

impl<P: ParseableTrade> Default for AddressTrades<P> {
    fn default() -> Self {
        Self {
            active: None,
            closed: Vec::new(),
        }
    }
}

