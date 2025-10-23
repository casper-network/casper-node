//! Types that can be safely shared between host and the wasm sdk.
use bitflags::bitflags;

bitflags! {
    /// Flags that can be passed as part of returning values.
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ReturnFlags: u32 {
        /// If this bit is set, the host should return the value to the caller and all the execution effects are reverted.
        const ROLLBACK = 0x0000_0001;
        /// If this bit is set, the host should abort the entire call stack.
        /// The optional return data is interpreted as a UTF-8 message.
        const REVERT = 0x0000_0002;
    }

    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct EntryPointFlags: u32 {
        const CONSTRUCTOR = 0x0000_0001;
    }
}

impl Default for EntryPointFlags {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_return_flags() {
        assert_eq!(ReturnFlags::empty().bits(), 0x0000_0000);
        assert_eq!(ReturnFlags::ROLLBACK.bits(), 0x0000_0001);
    }

    #[test]
    fn creating_from_invalid_bit_flags_does_not_fail() {
        let return_flags_ret_1 = ReturnFlags::from_bits(u32::MAX);
        assert_eq!(return_flags_ret_1, None);

        let maybe_rollback = ReturnFlags::from_bits(0x0000_0001);
        assert_eq!(maybe_rollback, Some(ReturnFlags::ROLLBACK));

        let maybe_empty = ReturnFlags::from_bits(0x0000_0000);
        assert_eq!(maybe_empty, Some(ReturnFlags::empty()));
    }
}
