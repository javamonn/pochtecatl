use super::dex::DexIndexedTrade;

use alloy::primitives::Address;

use fixed::types::U32F96;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct TickData {
    pub open: U32F96,
    pub high: U32F96,
    pub low: U32F96,
    pub close: U32F96,
    pub weth_volume: BigUint,
}

impl TickData {
    pub fn new(
        open: U32F96,
        high: U32F96,
        low: U32F96,
        close: U32F96,
        weth_volume: BigUint,
    ) -> Self {
        Self {
            open,
            high,
            low,
            close,
            weth_volume,
        }
    }

    pub fn add(&mut self, other: &Self) {
        if other.high > self.high {
            self.high = other.high
        }
        if other.low < self.low {
            self.low = other.low
        }
        self.close = other.close;
        self.weth_volume += other.weth_volume.clone();
    }

    pub fn reduce<'a>(open: Option<U32F96>, data: impl Iterator<Item = &'a Self>) -> Option<Self> {
        data.fold(None, |acc, price_bar| match acc {
            None => {
                let mut init = price_bar.clone();
                if let Some(open) = open {
                    init.open = open;
                }
                Some(init)
            }
            Some(mut acc) => {
                acc.add(price_bar);
                Some(acc)
            }
        })
    }

    pub fn is_negative(&self) -> bool {
        self.close < self.open
    }

    pub fn from_indexed_trade<T>(indexed_trade: &T, token_address: &Address) -> Self
    where
        T: DexIndexedTrade,
    {
        let price_before = indexed_trade.token_price_before(token_address);
        let price_after = indexed_trade.token_price_after(token_address);

        let (low, high) = if price_before < price_after {
            (price_before.clone(), price_after.clone())
        } else {
            (price_after.clone(), price_before.clone())
        };

        Self {
            open: price_before,
            close: price_after,
            high,
            low,
            weth_volume: indexed_trade.weth_volume(token_address).try_into().unwrap(),
        }
    }

    pub fn add_indexed_trade<T>(&mut self, indexed_trade: &T, token_address: &Address)
    where
        T: DexIndexedTrade,
    {
        let price = indexed_trade.token_price_after(token_address);
        if price > self.high {
            self.high = price.clone()
        } else if price < self.low {
            self.low = price.clone()
        }

        self.close = price;

        let indexed_trade_weth_volume: BigUint =
            indexed_trade.weth_volume(token_address).try_into().unwrap();
        self.weth_volume += indexed_trade_weth_volume;
    }
}

#[cfg(test)]
mod tests {
    use super::TickData;

    use crate::{DexIndexedTrade, UniswapV2IndexedTrade};

    use alloy::primitives::{address, uint, Address};

    #[test]
    fn test_from_uniswap_v2_pair() {
        let token_address = address!("F7669AC505D8Eb518103fEDa96A7A12737794492");
        let trades = vec![
            UniswapV2IndexedTrade::new(
                Address::ZERO,
                uint!(0_U256),
                uint!(196648594373849_U256),
                uint!(110094173315701195_U256),
                uint!(0_U256),
                uint!(24234363659908185248_U256),
                uint!(43353851609950831_U256),
                address!("1Fba6b0BBae2B74586fBA407Fb45Bd4788B7b130"),
            ),
            UniswapV2IndexedTrade::new(
                Address::ZERO,
                uint!(7500000000000000_U256),
                uint!(0_U256),
                uint!(0_U256),
                uint!(13372681690099_U256),
                uint!(24241863659908185248_U256),
                uint!(43340478928260732_U256),
                address!("7381C38985dA304eBA18fCef5E1f6e9fA0798b84"),
            ),
        ];

        let mut tick_data = TickData::from_indexed_trade(&trades[0], &token_address);
        tick_data.add_indexed_trade(&trades[1], &token_address);

        assert_eq!(tick_data.open, trades[0].token_price_before(&token_address));
        assert_eq!(tick_data.close, trades[1].token_price_after(&token_address));
        assert_eq!(tick_data.high, trades[0].token_price_before(&token_address));
        assert_eq!(tick_data.low, trades[0].token_price_after(&token_address));
    }
}
