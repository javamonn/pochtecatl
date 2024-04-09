use super::block_parser::UniswapV2Pair;

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
    pub fn add(&mut self, price: GenericFraction<u128>) {
        if price > self.high {
            self.high = price.clone()
        } else if price < self.low {
            self.low = price.clone()
        }
        self.close = price
    }

    pub fn from_uniswap_v2_pair(pair: &UniswapV2Pair) -> Option<Self> {
        pair.trades.iter().fold(None, |acc, pair_trade| match acc {
            None => {
                let price_before = pair_trade.get_price_before(&pair.token_address);
                let price_after = pair_trade.get_price_after(&pair.token_address);
                let (low, high) = if price_before < price_after {
                    (price_before, price_after)
                } else {
                    (price_after, price_before)
                };

                Some(Self {
                    open: price_before,
                    close: price_after,
                    high,
                    low,
                })
            }
            Some(mut price_bar) => {
                price_bar.add(pair_trade.get_price_after(&pair.token_address));
                Some(price_bar)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::BlockPriceBar;

    use crate::indexer::block_parser::{UniswapV2Pair, UniswapV2PairTrade};

    use alloy::primitives::{address, uint};

    #[test]
    fn test_from_uniswap_v2_pair() {
        let token_address = address!("F7669AC505D8Eb518103fEDa96A7A12737794492");
        let trades = vec![
            UniswapV2PairTrade::new(
                uint!(0_U256),
                uint!(196648594373849_U256),
                uint!(110094173315701195_U256),
                uint!(0_U256),
                uint!(24234363659908185248_U256),
                uint!(43353851609950831_U256),
                address!("1Fba6b0BBae2B74586fBA407Fb45Bd4788B7b130"),
            ),
            UniswapV2PairTrade::new(
                uint!(7500000000000000_U256),
                uint!(0_U256),
                uint!(0_U256),
                uint!(13372681690099_U256),
                uint!(24241863659908185248_U256),
                uint!(43340478928260732_U256),
                address!("7381C38985dA304eBA18fCef5E1f6e9fA0798b84"),
            ),
        ];

        let block_price_bar =
            BlockPriceBar::from_uniswap_v2_pair(&UniswapV2Pair::new(token_address, trades.clone()))
                .expect("Expected block_price_bar, but found None");

        assert_eq!(
            block_price_bar.open,
            trades[0].get_price_before(&token_address)
        );
        assert_eq!(
            block_price_bar.close,
            trades[1].get_price_after(&token_address)
        );
        assert_eq!(
            block_price_bar.high,
            trades[0].get_price_before(&token_address)
        );
        assert_eq!(
            block_price_bar.low,
            trades[0].get_price_after(&token_address)
        );
    }
}
