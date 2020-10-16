use num_rational::Ratio;

use crate::U512;

/// Returns the round seigniorage rate
///
/// Annual issuance: 2%
/// Minimum round exponent: 14
/// Ticks per year: 31536000000
///
/// ```python
/// >>> ((1+0.02)**(2**14/31536000000)-1)
/// 1.0288123020174567e-08
/// >>> 102881230202/10000000000000000000
/// 1.02881230202e-08
/// ```
#[inline]
pub fn round_seigniorage_rate() -> Ratio<U512> {
    const ROUND_SEIGNIORAGE_RATE_NUMER: u64 = 102881230202;
    const ROUND_SEIGNIORAGE_RATE_DENOM: u64 = 10_000_000_000_000_000_000;
    Ratio::new(
        U512::from(ROUND_SEIGNIORAGE_RATE_NUMER),
        U512::from(ROUND_SEIGNIORAGE_RATE_DENOM),
    )
}
