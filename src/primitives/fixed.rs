use alloy::primitives::{uint, U256, U512};

use fixed::FixedU128;
use lazy_static::lazy_static;

use fixed::types::U32F96;

lazy_static! {
    pub static ref FIXED_POINT_SCALE: U512 = uint!(1_U512) << 96;
    pub static ref INVERSE_FIXED_POINT_FACTOR: U512 = uint!(2_U512).pow(uint!(288_U512));
    pub static ref TWO: U512 = uint!(2_U512);
}

// Q32.96 fixed-point number, inspired by uniswap v3 pricing.
//
// Since we price in terms of WETH, the vast majority of pairs will use the decimal
// portion only.

pub fn u32f96_from_u256_frac(numerator: U256, denominator: U256) -> U32F96 {
    let scaled_numerator = U512::from(numerator)
        .checked_mul(*FIXED_POINT_SCALE)
        .expect("overflow while scaling numerator");

    let frac = scaled_numerator / U512::from(denominator);
    let bytes = frac
        .as_le_slice()
        .get(0..16)
        .expect("expected at least 16 bytes");

    let mut own = [0u8; 16];
    own.copy_from_slice(bytes);

    FixedU128::from_le_bytes(own)
}

pub fn u32f96_from_sqrt_x96(sqrt_price_x96: U256, inverse: bool) -> U32F96 {
    if inverse {
        let r = *INVERSE_FIXED_POINT_FACTOR / U512::from(sqrt_price_x96).pow(*TWO);
        let bytes = r
            .as_le_slice()
            .get(0..16)
            .expect("expected at least 16 bytes");
        let mut own = [0u8; 16];
        own.copy_from_slice(bytes);
        U32F96::from_le_bytes(own)
    } else {
        let bytes = sqrt_price_x96
            .as_le_slice()
            .get(0..16)
            .expect("expected at least 16 bytes");
        let mut own = [0u8; 16];
        own.copy_from_slice(bytes);
        let sqrt_price = U32F96::from_le_bytes(own);

        sqrt_price * sqrt_price
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use alloy::primitives::uint;

    #[test]
    fn test_from_u256_frac() {
        let price = u32f96_from_u256_frac(U256::from(1), U256::from(3));

        assert_eq!(price.to_string(), "0.33333333333333333333333333333");
    }

    #[test]
    fn test_from_sqrt_x96() {
        // test pricing, where the price value from uniswap is in terms of weth
        assert_eq!(
            u32f96_from_sqrt_x96(uint!(5850663725952942981099018_U256), false).to_string(),
            "0.00000000545319599349665909099"
        );
        assert_eq!(
            u32f96_from_sqrt_x96(uint!(1202044578294812206020225333_U256), false).to_string(),
            "0.00023018762943770319237307297"
        );

        // test inverse pricing where the price value from uniswap is in terms of token and we want
        // pricing in weth
        assert_eq!(
            u32f96_from_sqrt_x96(uint!(33480272181862439479917265759682_U256), true).to_string(),
            "0.00000559991206693093302143302"
        );
        assert_eq!(
            u32f96_from_sqrt_x96(uint!(3669259994132435458437044424_U256), true).to_string(),
            "466.23212634818300329104199501172"
        );
    }
}
