//! Support for memory shared objects with behavior that can be switched at runtime.

use std::{fmt::Display, ops::Deref, sync::Arc};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// An in-memory object that can possibly be shared with other parts of the system.
///
/// In general, this should only be used for immutable, content-addressed objects.
///
/// This type exists solely to switch between `Box` and `Arc` based behavior, future updates should
/// deprecate this in favor of using `Arc`s directly or turning `SharedObject` into a newtype.
#[derive(Clone, DataSize, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SharedObject<T> {
    /// An owned copy of the object.
    Owned(Box<T>),
    /// A shared copy of the object.
    Shared(Arc<T>),
}

impl<T> Deref for SharedObject<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            SharedObject::Owned(obj) => &*obj,
            SharedObject::Shared(shared) => &*shared,
        }
    }
}

impl<T> AsRef<[u8]> for SharedObject<T>
where
    T: AsRef<[u8]>,
{
    fn as_ref(&self) -> &[u8] {
        match self {
            SharedObject::Owned(obj) => <T as AsRef<[u8]>>::as_ref(obj),
            SharedObject::Shared(shared) => <T as AsRef<[u8]>>::as_ref(shared),
        }
    }
}

impl<T> SharedObject<T> {
    /// Creates a new owned instance of the object.
    #[inline]
    pub(crate) fn owned(inner: T) -> Self {
        SharedObject::Owned(Box::new(inner))
    }

    /// Creates a new shared instance of the object.
    #[allow(unused)] // TODO[RC]: Used only in the mem deduplication feature (via ` fn
                     // handle_deduplicated_legacy_direct_deploy_request(deploy_hash)`), which is not merged from
                     // `dev` to `feat-fast-sync` (?)
    pub(crate) fn shared(inner: Arc<T>) -> Self {
        SharedObject::Shared(inner)
    }
}

impl<T> Display for SharedObject<T>
where
    T: Display,
{
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SharedObject::Owned(inner) => inner.fmt(f),
            SharedObject::Shared(inner) => inner.fmt(f),
        }
    }
}

impl<T> Serialize for SharedObject<T>
where
    T: Serialize,
{
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SharedObject::Owned(inner) => inner.serialize(serializer),
            SharedObject::Shared(shared) => shared.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for SharedObject<T>
where
    T: Deserialize<'de>,
{
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize(deserializer).map(SharedObject::owned)
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use serde::{de::DeserializeOwned, Serialize};

    use crate::types::Deploy;

    use super::SharedObject;

    impl<T> SharedObject<T>
    where
        T: Clone,
    {
        pub(crate) fn into_inner(self) -> T {
            match self {
                SharedObject::Owned(inner) => *inner,
                SharedObject::Shared(shared) => (*shared).clone(),
            }
        }
    }

    // TODO: Import fixed serialization settings from `small_network::message_pack_format` as soon
    //       as merged, instead of using these `rmp_serde` helper functions.
    #[inline]
    fn serialize<T: Serialize>(value: &T) -> Vec<u8> {
        rmp_serde::to_vec(value).expect("could not serialize value")
    }

    #[inline]
    fn deserialize<T: DeserializeOwned>(raw: &[u8]) -> T {
        rmp_serde::from_read(Cursor::new(raw)).expect("could not deserialize value")
    }

    #[test]
    fn loaded_item_for_bytes_deserializes_like_bytevec() {
        // Construct an example payload that is reasonably realistic.
        let mut rng = crate::new_rng();
        let deploy = Deploy::random(&mut rng);
        let payload = bincode::serialize(&deploy).expect("could not serialize deploy");

        // Realistic payload inside a `GetRequest`.
        let loaded_item_owned = SharedObject::owned(payload.clone());
        let loaded_item_shared = SharedObject::shared(Arc::new(payload.clone()));

        // Check all serialize the same.
        let serialized = serialize(&payload);
        assert_eq!(serialized, serialize(&loaded_item_owned));
        assert_eq!(serialized, serialize(&loaded_item_shared));

        // Ensure we can deserialize a loaded item payload.
        let deserialized: SharedObject<Vec<u8>> = deserialize(&serialized);

        assert_eq!(payload, deserialized.into_inner());
    }
}
