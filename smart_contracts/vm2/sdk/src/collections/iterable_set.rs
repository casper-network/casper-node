#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
use crate::abi::{ABIVisitor, AbiDeclaration, CasperABI, Definition};
use borsh::{BorshDeserialize, BorshSerialize};
use casper_executor_wasm_common::type_uid::{TypeUid, Uid};

use super::{IterableMap, IterableMapHash};
use crate::{compat::types::CLTyped, prelude::String};

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
/// An iterable set backed by a map.
pub struct IterableSet<V> {
    pub(crate) map: IterableMap<V, ()>,
}

impl<V: IterableMapHash + BorshSerialize + BorshDeserialize + Clone> IterableSet<V> {
    /// Creates an empty [IterableMap] with the given prefix.
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        Self {
            map: IterableMap::new(prefix),
        }
    }

    /// Inserts a value into the set.
    pub fn insert(&mut self, value: V) {
        self.map.insert(value, ());
    }

    /// Removes a value from the set.
    ///
    /// Has a worst-case runtime of O(n).
    pub fn remove(&mut self, value: &V) {
        self.map.remove(value);
    }

    /// Returns true if the set contains a value.
    pub fn contains(&self, value: &V) -> bool {
        self.map.get(value).is_some()
    }

    /// Creates an iterator visiting all the values in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = V> + '_ {
        self.map.iter().map(|(value, _)| value)
    }

    // Returns true if the set contains no elements.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Clears the set, removing all values.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

impl<V> TypeUid for IterableSet<V>
where
    V: TypeUid,
{
    const UID: Uid = Uid::from_fields("IterabeSet", &[V::UID]);
}

impl<V: CLTyped> CLTyped for IterableSet<V> {
    fn cl_type() -> crate::compat::types::CLType {
        crate::compat::types::CLType::Any
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
impl<V: CasperABI> CasperABI for IterableSet<V> {
    fn visit(visitor: &mut dyn ABIVisitor) {
        V::visit(visitor);
    }

    fn declaration() -> AbiDeclaration {
        format!("IterableSet<{}>", V::declaration())
    }

    #[inline]
    fn definition() -> Definition {
        use crate::abi::StructField;

        Definition::Struct {
            items: vec![StructField {
                name: "map".into(),
                decl: casper_executor_wasm_common::type_uid::of::<IterableMap<V, ()>>().into(),
            }],
        }
    }
}
