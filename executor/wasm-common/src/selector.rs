use std::{
    num::NonZeroU32,
    ops::{BitXor, BitXorAssign},
};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Selector(u32);

impl From<NonZeroU32> for Selector {
    fn from(selector: NonZeroU32) -> Self {
        Selector(selector.get())
    }
}

impl BitXorAssign for Selector {
    fn bitxor_assign(&mut self, rhs: Self) {
        self.0 ^= rhs.0;
    }
}

impl BitXor for Selector {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Selector(self.0 ^ rhs.0)
    }
}

impl Selector {
    pub const fn zero() -> Self {
        Selector(0)
    }
    pub const fn new(selector: u32) -> Self {
        Selector(selector)
    }

    pub const fn get(&self) -> u32 {
        self.0
    }

    pub const fn xor(&self, other: Self) -> Self {
        Selector(self.0 ^ other.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::selector::Selector;
    const SELECTOR1: Selector = Selector::new(0x1234_5678);
    const SELECTOR2: Selector = Selector::new(0x8765_4321);
    const COMBINED_SELECTOR: Selector = SELECTOR1.xor(SELECTOR2);

    #[test]
    fn test_selector() {
        let selector3 = SELECTOR1 ^ SELECTOR2;
        assert_eq!(selector3, Selector::new(0x9551_1559));
        assert_eq!(selector3, COMBINED_SELECTOR);
    }

    #[test]
    fn combine() {
        let mut combined = Selector::zero();
        combined ^= SELECTOR2;
        combined ^= SELECTOR1;
        assert_eq!(combined, COMBINED_SELECTOR);
        assert_eq!(combined ^ SELECTOR2, SELECTOR1);
        assert_eq!(combined ^ SELECTOR1, SELECTOR2);
    }
}
