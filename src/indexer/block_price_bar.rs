use super::log_parser::UniswapV2PairTrade;
use crate::config;

use alloy::primitives::Address;

use fraction::GenericFraction;

// Smallest price unit
// Denominated in WETH
#[derive(Clone, Copy)]
pub struct BlockPriceBar {
    pub open: GenericFraction<u128>,
    pub high: GenericFraction<u128>,
    pub low: GenericFraction<u128>,
    pub close: GenericFraction<u128>,
}

impl BlockPriceBar {
    pub fn new(open: GenericFraction<u128>) -> Self {
        Self {
            open: open.clone(),
            high: open.clone(),
            low: open.clone(),
            close: open,
        }
    }

    pub fn add(&mut self, price: GenericFraction<u128>) {
        if price > self.high {
            self.high = price.clone()
        } else if price < self.low {
            self.low = price.clone()
        }
        self.close = price
    }

    pub fn from_uniswap_v2_trades(
        trades: &Vec<UniswapV2PairTrade>,
        token0_address: &Address,
        token1_address: &Address,
    ) -> Option<Self> {
        trades
            .iter()
            .filter_map(|t| {
                if *token0_address == *config::WETH_ADDRESS {
                    Some(GenericFraction::new(
                        t.reserve0.to::<u128>(),
                        t.reserve1.to::<u128>(),
                    ))
                } else if *token1_address == *config::WETH_ADDRESS {
                    Some(GenericFraction::new(
                        t.reserve1.to::<u128>(),
                        t.reserve0.to::<u128>(),
                    ))
                } else {
                    None
                }
            })
            .fold(None, |acc, price| match acc {
                None => Some(BlockPriceBar::new(price)),
                Some(mut price_bar) => {
                    price_bar.add(price);
                    Some(price_bar)
                }
            })
    }
}
