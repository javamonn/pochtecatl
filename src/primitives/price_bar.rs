use alloy::primitives::BlockNumber;
use fraction::GenericFraction;
use std::collections::BTreeMap;

pub type F = fraction::GenericFraction<u128>;

// 5 minutes
pub const RESOLUTION: u64 = 60 * 5;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct PriceBarTimestamp(u64);

impl PriceBarTimestamp {
    pub fn count_ticks(start: &PriceBarTimestamp, end: &PriceBarTimestamp) -> u64 {
        end.0 - start.0 / RESOLUTION
    }
}

pub struct PriceBarData {
    pub open: GenericFraction<u128>,
    pub high: GenericFraction<u128>,
    pub low: GenericFraction<u128>,
    pub close: GenericFraction<u128>,
}

impl PriceBarData {
    pub fn from_trades_by_block_number(
        /*
        trades_by_block_number: &BTreeMap<BlockNumber, Vec<Trade>>,
        */
    ) -> Self {
        /*
        trades_by_block_number.values().flatten().fold(None, |price_bar_data, trades| {
            match price_bar_data {
                Some(PriceBarData { open, high, low, close }) => {
                    Some(PriceBarData {
                        open,
                        high: 
                    })
                }
            }
        })
        */
        unimplemented!()
    }
}

/*
pub struct PriceBar {
    ts: PriceBarTimestamp,
    data: Option<PriceBarData>,
    trades_by_block_number: BTreeMap<BlockNumber, Vec<Trade>>,
}

fn compute_price_bar_data(
    trades_by_block_number: &BTreeMap<BlockNumber, Vec<Trade>>,
) -> PriceBarData {
    unimplemented!()
}

impl PriceBar {
    pub fn insert_trades(&mut self, block_number: BlockNumber, trades: Vec<Trade>) {
        // Data is invalidated by new trades
        self.data = None;
        self.trades_by_block_number.insert(block_number, trades);
    }

    pub fn get_data(&mut self) -> &PriceBarData {
        match self.data {
            Some(ref data) => data,
            None => {
                let data = compute_price_bar_data(&self.trades_by_block_number);
                self.data = Some(data);
                self.get_data()
            }
        }
    }
}
*/

pub struct PriceBar {}
