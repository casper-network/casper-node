//! Types that can be safely shared between host and the wasm sdk.
use bitflags::bitflags;

bitflags! {
    /// Flags that can be passed as part of returning values.
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ReturnFlags: u32 {
        /// If this bit is set, the host should return the value to the caller and all the execution effects are reverted.
        const REVERT = 0x0000_0001;

        // The source may set any bits.
        const _ = !0;
    }

    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct EntryPointFlags: u32 {
        const CONSTRUCTOR = 0x0000_0001;
        const FALLBACK = 0x0000_0002;
    }

    /// Flags that can be passed as part of calling contracts.
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct CallFlags: u32 {
        // TODO: This is a placeholder
    }
}

impl Default for EntryPointFlags {
    fn default() -> Self {
        Self::empty()
    }
}

impl Default for CallFlags {
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
        assert_eq!(ReturnFlags::REVERT.bits(), 0x0000_0001);
    }

    #[test]
    fn creating_from_invalid_bit_flags_does_not_fail() {
        let _return_flags = ReturnFlags::from_bits(u32::MAX).unwrap();
        let _revert = ReturnFlags::from_bits(0x0000_0001).unwrap();
        let _empty = ReturnFlags::from_bits(0x0000_0000).unwrap();
    }
}
