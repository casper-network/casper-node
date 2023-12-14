#[repr(u32)]
pub enum Keyspace<'a> {
    /// Stores contract's context
    State = 0,
    /// Stores contract's context date. Bytes can be any value as long as it uniquely identifies a
    /// value.
    Context(&'a [u8]) = 1,
}
