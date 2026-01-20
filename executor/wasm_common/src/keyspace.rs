use borsh::{BorshDeserialize, BorshSerialize};
use num_derive::{FromPrimitive, ToPrimitive};

use crate::type_uid::Uid;

/// Discriminant indicating which keyspace is being accessed.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum KeyspaceTag {
    /// Context-based storage addressing using a structured address.
    Context = 0,
    /// Named key based storage which usually involves human-readable names.
    NamedKey = 1,
    /// Used for type definitions.
    TypeDef = 2,
    /// Used for entry points.
    EntryPoint = 3,
}

/// Discriminant indicating which collection type is being used.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum CollectionTypeTag {
    /// A key-value mapping collection.
    Map = 0,
    /// A set collection for unique elements.
    Set = 1,
    /// A vector collection.
    Vector = 2,
    /// An iterable map collection.
    IterableMap = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ContextAddr {
    /// Address of a state field for a given entity.
    StateAddr(StateAddrInner),
    /// Address of a collection element for a given entity.
    CollectionAddr(CollectionAddrInner),
}

/// Address for a specific state field owned by `entity_addr`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StateAddrInner {
    pub field_addr: String,
}

impl StateAddrInner {
    #[inline]
    pub fn new<T: Into<String>>(field_addr: T) -> Self {
        Self {
            field_addr: field_addr.into(),
        }
    }
}

/// Address for a collection element owned by `entity_addr`.
///
/// The `collection_type_tag` identifies the collection kind (e.g., map, set, vector).
/// The `collection_prefix` is an 8-byte collection-level namespace derived from the collection
/// name. The `tail` is a 32-byte element-level discriminator (e.g., hashed key or index).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollectionAddrInner {
    pub entity_addr: [u8; 32],
    pub collection_type_tag: u8,
    pub collection_prefix: [u8; 8],
    pub tail: [u8; 32],
}

impl CollectionAddrInner {
    pub fn new(
        entity_addr: [u8; 32],
        collection_type_tag: CollectionTypeTag,
        collection_prefix: [u8; 8],
        tail: [u8; 32],
    ) -> Self {
        Self {
            entity_addr,
            collection_type_tag: collection_type_tag as u8,
            collection_prefix,
            tail,
        }
    }
}

impl From<StateAddrInner> for ContextAddr {
    fn from(value: StateAddrInner) -> Self {
        ContextAddr::StateAddr(value)
    }
}

impl From<CollectionAddrInner> for ContextAddr {
    fn from(value: CollectionAddrInner) -> Self {
        ContextAddr::CollectionAddr(value)
    }
}

#[repr(u64)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keyspace<'a> {
    /// Structured context address.
    Context(ContextAddr),
    /// Human-readable named key.
    NamedValue(&'a str),
    /// Retrieves contract's type definitions.
    TypeDef(Uid),
    /// Stores contract's entry points.
    EntryPoint(&'a str),
}

impl Keyspace<'_> {
    #[must_use]
    pub fn as_tag(&self) -> KeyspaceTag {
        match self {
            Keyspace::Context(_) => KeyspaceTag::Context,
            Keyspace::NamedValue(_) => KeyspaceTag::NamedKey,
            Keyspace::TypeDef(_) => KeyspaceTag::TypeDef,
            Keyspace::EntryPoint(_) => KeyspaceTag::EntryPoint,
        }
    }

    #[must_use]
    pub fn as_u64(&self) -> u64 {
        self.as_tag() as u64
    }

    pub fn to_host_input_data(&self) -> borsh::io::Result<Vec<u8>> {
        match self {
            Keyspace::Context(key_bytes) => {
                borsh::to_vec(&(KeyspaceTag::Context as u64, borsh::to_vec(key_bytes)?))
            }
            Keyspace::TypeDef(typedef) => borsh::to_vec(&(
                KeyspaceTag::TypeDef as u64,
                typedef.into_raw().to_le_bytes().to_vec(),
            )),
            Keyspace::EntryPoint(entry_point_name) => {
                borsh::to_vec(&(KeyspaceTag::EntryPoint as u64, entry_point_name))
            }
            Keyspace::NamedValue(key_bytes) => {
                borsh::to_vec(&(KeyspaceTag::NamedKey as u64, key_bytes.as_bytes()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::type_uid::Uid;

    use super::*;

    #[test]
    fn test_as_tag_context() {
        let context_addr = ContextAddr::StateAddr(StateAddrInner::new("abc"));
        let keyspace = Keyspace::Context(context_addr);
        assert_eq!(keyspace.as_tag(), KeyspaceTag::Context);
    }

    #[test]
    fn test_as_tag_named_key() {
        let name = "my_key";
        let keyspace = Keyspace::NamedValue(name);
        assert_eq!(keyspace.as_tag(), KeyspaceTag::NamedKey);
    }

    #[test]
    fn test_as_u64_context() {
        let context_addr = ContextAddr::StateAddr(StateAddrInner::new("abc"));
        let keyspace = Keyspace::Context(context_addr);
        assert_eq!(keyspace.as_u64(), 0);
    }

    #[test]
    fn test_as_u64_named_key() {
        let name = "my_key";
        let keyspace = Keyspace::NamedValue(name);
        assert_eq!(keyspace.as_u64(), 2);
    }

    #[test]
    fn test_as_u64_type_def() {
        let keyspace = Keyspace::TypeDef(Uid::from_name("foobar"));
        assert_eq!(keyspace.as_u64(), 3);
    }

    #[test]
    fn test_as_u64_entry_point() {
        let name = "my_entry_point";
        let keyspace = Keyspace::EntryPoint(name);
        assert_eq!(keyspace.as_u64(), 4);
    }
}
