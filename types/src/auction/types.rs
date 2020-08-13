/// Representation of delegation rate of tokens. Fraction of 1 in trillionths (12 decimal places).
pub type CommissionRate = u64;

/// Commission rate is a fraction between 0-1. Validator sets the commission rate 
/// in integer terms, which is then divided by the denominator to obtain the fraction.
pub const COMMISSION_RATE_DENOMINATOR: u64 = 1_000_000_000_000;
