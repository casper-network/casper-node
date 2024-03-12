#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyspace<'a> {
    /// Stores contract's context.
    ///
    /// There's no additional payload for this variant as the host implies the contract's address.
    State = Keyspace::KEYSPACE_STATE,
    /// Stores contract's context date. Bytes can be any value as long as it uniquely identifies a
    /// value.
    Context(&'a [u8]) = Keyspace::KEYSPACE_CONTEXT,
}

impl<'a> Keyspace<'a> {
    /// Used for a state based storage which usually involves single dimensional data i.e. key-value pairs, etc.
    ///
    /// See also [`Keyspace::State`].
    pub const KEYSPACE_STATE: u64 = 0;
    /// Used for a context based storż/// Used for a context based storage which usually involves multi dimensional data i.e. maps, efficient vectors, etc.
    pub const KEYSPACE_CONTEXT: u64 = 1;

    pub fn from_raw_parts(keyspace: u64, payload: &'a [u8]) -> Option<Self> {
        match keyspace {
            Self::KEYSPACE_STATE => Some(Keyspace::State),
            Self::KEYSPACE_CONTEXT => Some(Keyspace::Context(payload)),
            _ => None,
        }
    }
    /// Returns the raw value of the enum variant.
    pub fn as_u64(&self) -> u64 {
        match self {
            Keyspace::State => Self::KEYSPACE_STATE,
            Keyspace::Context(_) => Self::KEYSPACE_CONTEXT,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_raw_parts_state() {
        let keyspace = Keyspace::from_raw_parts(Keyspace::KEYSPACE_STATE, &[]);
        assert_eq!(keyspace, Some(Keyspace::State));
    }

    #[test]
    fn test_from_raw_parts_context() {
        let payload = &[1, 2, 3];
        let keyspace = Keyspace::from_raw_parts(Keyspace::KEYSPACE_CONTEXT, payload);
        assert_eq!(keyspace, Some(Keyspace::Context(payload)));
    }

    #[test]
    fn test_from_raw_parts_invalid() {
        let keyspace = Keyspace::from_raw_parts(u64::MAX, &[1, 2, 3, 4, 5]);
        assert_eq!(keyspace, None);
    }

    #[test]
    fn test_as_u64_state() {
        let keyspace = Keyspace::State;
        assert_eq!(keyspace.as_u64(), Keyspace::KEYSPACE_STATE);
    }

    #[test]
    fn test_as_u64_context() {
        let payload = &[1, 2, 3];
        let keyspace = Keyspace::Context(payload);
        assert_eq!(keyspace.as_u64(), Keyspace::KEYSPACE_CONTEXT);
    }
}
