#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Selector(u32);

impl Selector {
    pub const fn new(selector: u32) -> Self {
        Selector(selector)
    }

    pub const fn get(&self) -> u32 {
        self.0
    }
}
