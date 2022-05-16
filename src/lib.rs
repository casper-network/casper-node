pub mod chunked;
pub mod length_prefixed;

use bytes::Buf;

pub struct ImmediateFrame<A> {
    pos: usize,
    value: A,
}

impl<A> ImmediateFrame<A> {
    #[inline]
    pub fn new(value: A) -> Self {
        Self { pos: 0, value }
    }
}

impl From<u8> for ImmediateFrame<[u8; 1]> {
    #[inline]
    fn from(value: u8) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl From<u16> for ImmediateFrame<[u8; 2]> {
    #[inline]
    fn from(value: u16) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl From<u32> for ImmediateFrame<[u8; 4]> {
    #[inline]
    fn from(value: u32) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl<A> Buf for ImmediateFrame<A>
where
    A: AsRef<[u8]>,
{
    fn remaining(&self) -> usize {
        // Does not overflow, as `pos` is  `< .len()`.

        self.value.as_ref().len() - self.pos
    }

    fn chunk(&self) -> &[u8] {
        // Safe access, as `pos` is guaranteed to be `< .len()`.
        &self.value.as_ref()[self.pos..]
    }

    fn advance(&mut self, cnt: usize) {
        // This is the only function modifying `pos`, upholding the invariant of it being smaller
        // than the length of the data we have.
        self.pos = (self.pos + cnt).min(self.value.as_ref().len());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Read;

    use bytes::Buf;

    /// Collects everything inside a `Buf` into a `Vec`.
    pub fn collect_buf<B: Buf>(buf: B) -> Vec<u8> {
        let mut vec = Vec::new();
        buf.reader()
            .read_to_end(&mut vec)
            .expect("reading buf should never fail");
        vec
    }
}
