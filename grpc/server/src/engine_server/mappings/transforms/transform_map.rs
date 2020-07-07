use std::convert::{TryFrom, TryInto};

use node::contract_shared::{additive_map::AdditiveMap, transform::Transform};
use types::Key;

use crate::engine_server::{mappings::ParsingError, transforms::TransformEntry};

pub struct TransformMap(AdditiveMap<Key, Transform>);

impl TransformMap {
    pub fn into_inner(self) -> AdditiveMap<Key, Transform> {
        self.0
    }
}

impl TryFrom<Vec<TransformEntry>> for TransformMap {
    type Error = ParsingError;

    fn try_from(pb_transform_map: Vec<TransformEntry>) -> Result<Self, Self::Error> {
        let mut transforms_merged: AdditiveMap<Key, Transform> = AdditiveMap::new();
        for pb_transform_entry in pb_transform_map {
            let (key, transform) = pb_transform_entry.try_into()?;
            transforms_merged.insert_add(key, transform);
        }
        Ok(TransformMap(transforms_merged))
    }
}

#[cfg(test)]
mod tests {
    use node::contract_shared::stored_value::StoredValue;
    use types::CLValue;

    use super::*;

    #[test]
    fn commit_effects_merges_transforms() {
        // Tests that transforms made to the same key are merged instead of lost.
        let key = Key::Hash([1u8; 32]);
        let setup: Vec<TransformEntry> = {
            let transform_entry_first = {
                let mut tmp = TransformEntry::new();
                tmp.set_key(key.into());
                tmp.set_transform(
                    Transform::Write(StoredValue::CLValue(CLValue::from_t(12_i32).unwrap())).into(),
                );
                tmp
            };
            let transform_entry_second = {
                let mut tmp = TransformEntry::new();
                tmp.set_key(key.into());
                tmp.set_transform(Transform::AddInt32(10).into());
                tmp
            };
            vec![transform_entry_first, transform_entry_second]
        };
        let commit: TransformMap = setup
            .try_into()
            .expect("Transforming `Vec<TransformEntry>` into `TransformMap` should work.");
        let expected_transform =
            Transform::Write(StoredValue::CLValue(CLValue::from_t(22_i32).unwrap()));
        let commit_transform = commit.0.get(&key);
        assert!(commit_transform.is_some());
        assert_eq!(expected_transform, *commit_transform.unwrap())
    }
}
