/// The caller must cover cost.
///
/// This is the default mode in VM2 runtime.
pub const ENTRY_POINT_PAYMENT_CALLER: u8 = 0;
/// Will cover cost to execute self but not cost of any subsequent invoked contracts
pub const ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY: u8 = 1;
/// will cover cost to execute self and the cost of any subsequent invoked contracts
pub const ENTRY_POINT_PAYMENT_SELF_ONWARD: u8 = 2;
