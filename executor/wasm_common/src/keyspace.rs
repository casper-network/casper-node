use num_derive::{FromPrimitive, ToPrimitive};
use borsh::{BorshDeserialize, BorshSerialize};

/// Discriminant indicating which keyspace is being accessed.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum KeyspaceTag {
    /// Context-based storage addressing using a structured address.
    Context = 0,
    /// Named key based storage which usually involves human-readable names.
    NamedKey = 1,
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
    pub entity_addr: [u8; 32],
    pub field_addr: String,
}

impl StateAddrInner {
    #[inline]
    pub fn new<T: Into<String>>(entity_addr: [u8; 32], field_addr: T) -> Self {
        Self {
            entity_addr,
            field_addr: field_addr.into(),
        }
    }
}

/// Address for a collection element owned by `entity_addr`.
///
/// The `collection_type_tag` identifies the collection kind (e.g., map, set, vector).
/// The `collection_prefix` is an 8-byte collection-level namespace derived from the collection name.
/// The `tail` is a 32-byte element-level discriminator (e.g., hashed key or index).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollectionAddrInner {
    pub entity_addr: [u8; 32],
    pub collection_type_tag: u8,
    pub collection_prefix: [u8; 8],
    pub tail: [u8; 32],
}

impl CollectionAddrInner {
    #[inline]
    pub fn new(
        entity_addr: [u8; 32],
        collection_type_tag: u8,
        collection_prefix: [u8; 8],
        tail: [u8; 32],
    ) -> Self {
        Self {
            entity_addr,
            collection_type_tag,
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
    NamedKey(&'a str),
}

impl Keyspace<'_> {
    #[must_use]
    pub fn as_tag(&self) -> KeyspaceTag {
        match self {
            Keyspace::Context(_) => KeyspaceTag::Context,
            Keyspace::NamedKey(_) => KeyspaceTag::NamedKey,
        }
    }

    #[must_use]
    pub fn as_u64(&self) -> u64 {
        self.as_tag() as u64
    }
}
