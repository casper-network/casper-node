use num_derive::{FromPrimitive, ToPrimitive};

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum KeyspaceTag {
    /// Used for a state based storage which usually involves single dimensional data i.e.
    /// key-value pairs, etc.
    ///
    /// See also [`Keyspace::State`].
    State = 0,
    /// Used for a context based storage which usually involves multi dimensional data i.e. maps,
    /// efficient vectors, etc.
    Context = 1,
    /// Used for a named key based storage which usually involves named keys.
    NamedKey = 2,
    /// Used for a payment info based storage which usually involves payment information.
    PaymentInfo = 3,
}

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyspace<'a> {
    /// Stores contract's context.
    ///
    /// There's no additional payload for this variant as the host implies the contract's address.
    State,
    /// Stores contract's context date. Bytes can be any value as long as it uniquely identifies a
    /// value.
    Context(&'a [u8]),
    /// Stores contract's named keys.
    NamedKey(&'a str),
    /// Entry point payment info.
    PaymentInfo(&'a str),
}

impl<'a> Keyspace<'a> {
    pub fn as_tag(&self) -> KeyspaceTag {
        match self {
            Keyspace::State => KeyspaceTag::State,
            Keyspace::Context(_) => KeyspaceTag::Context,
            Keyspace::NamedKey(_) => KeyspaceTag::NamedKey,
            Keyspace::PaymentInfo(_) => KeyspaceTag::PaymentInfo,
        }
    }

    pub fn as_u64(&self) -> u64 {
        self.as_tag() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_tag_state() {
        let keyspace = Keyspace::State;
        assert_eq!(keyspace.as_tag(), KeyspaceTag::State);
    }

    #[test]
    fn test_as_tag_context() {
        let data = [1, 2, 3];
        let keyspace = Keyspace::Context(&data);
        assert_eq!(keyspace.as_tag(), KeyspaceTag::Context);
    }

    #[test]
    fn test_as_tag_named_key() {
        let name = "my_key";
        let keyspace = Keyspace::NamedKey(name);
        assert_eq!(keyspace.as_tag(), KeyspaceTag::NamedKey);
    }

    #[test]
    fn test_as_u64_state() {
        let keyspace = Keyspace::State;
        assert_eq!(keyspace.as_u64(), 0);
    }

    #[test]
    fn test_as_u64_context() {
        let data = [1, 2, 3];
        let keyspace = Keyspace::Context(&data);
        assert_eq!(keyspace.as_u64(), 1);
    }

    #[test]
    fn test_as_u64_named_key() {
        let name = "my_key";
        let keyspace = Keyspace::NamedKey(name);
        assert_eq!(keyspace.as_u64(), 2);
    }

    #[test]
    fn test_as_u64_payment_info() {
        let name = "entry_point";
        let keyspace: Keyspace = Keyspace::PaymentInfo(name);
        assert_eq!(keyspace.as_u64(), 3);
    }
}
