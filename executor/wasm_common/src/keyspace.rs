use num_derive::{FromPrimitive, ToPrimitive};

use crate::type_uid::Uid;

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
    /// Used for getting all named keys
    AllNamedKeys = 3,
    /// Used for type definitions.
    TypeDef = 4,
    /// Used for entry points.
    EntryPoint = 5,
}

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyspace<'a> {
    /// Stores contract's context.
    ///
    /// There's no additional payload for this variant as the host implies the contract's address.
    State,
    /// Stores contract's context data. Bytes can be any value as long as it uniquely identifies a
    /// value.
    Context(&'a [u8]),
    /// Stores contract's named keys.
    NamedKey(&'a str),
    /// All the named keys for the given contract
    ///
    /// No additional info as the contracts address will be used as the base.
    AllNamedKeys,
    /// Retrieves contract's type definitions.
    TypeDef(Uid),
    /// Stores contract's entry points.
    EntryPoint(&'a str),
}

const EMPTY_SLICE: &[u8] = &[];

impl Keyspace<'_> {
    pub fn to_host_input_data(&self) -> borsh::io::Result<Vec<u8>> {
        match self {
            Keyspace::State => borsh::to_vec(&(KeyspaceTag::State as u64, EMPTY_SLICE)),
            Keyspace::Context(key_bytes) => {
                borsh::to_vec(&(KeyspaceTag::Context as u64, key_bytes))
            }
            Keyspace::NamedKey(key_bytes) => {
                borsh::to_vec(&(KeyspaceTag::NamedKey as u64, key_bytes.as_bytes()))
            }
            Keyspace::AllNamedKeys => borsh::to_vec(&(KeyspaceTag::AllNamedKeys as u64,)),
            Keyspace::TypeDef(typedef) => borsh::to_vec(&(
                KeyspaceTag::TypeDef as u64,
                &typedef.into_raw().to_le_bytes()[..],
            )),
            Keyspace::EntryPoint(entry_point_name) => {
                borsh::to_vec(&(KeyspaceTag::EntryPoint as u64, entry_point_name.as_bytes()))
            }
        }
    }
}

impl Keyspace<'_> {
    #[must_use]
    pub fn as_tag(&self) -> KeyspaceTag {
        match self {
            Keyspace::State => KeyspaceTag::State,
            Keyspace::Context(_) => KeyspaceTag::Context,
            Keyspace::NamedKey(_) => KeyspaceTag::NamedKey,
            Keyspace::AllNamedKeys => KeyspaceTag::AllNamedKeys,
            Keyspace::TypeDef(_) => KeyspaceTag::TypeDef,
            Keyspace::EntryPoint(_) => KeyspaceTag::EntryPoint,
        }
    }

    #[must_use]
    pub fn as_u64(&self) -> u64 {
        self.as_tag() as u64
    }
}

#[cfg(test)]
mod tests {
    use crate::type_uid::Uid;

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
    fn test_as_u64_all_named_keys() {
        let keyspace = Keyspace::TypeDef(Uid::from_name("foobar"));
        assert_eq!(keyspace.as_u64(), 4);
    }

    #[test]
    fn test_as_u64_entry_point() {
        let name = "my_entry_point";
        let keyspace = Keyspace::EntryPoint(name);
        assert_eq!(keyspace.as_u64(), 5);
    }
}
