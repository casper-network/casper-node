use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
};

use casper_types::{contracts::NamedKeys, Key};

use crate::engine_server::{mappings::ParsingError, state::NamedKey};

impl From<(String, Key)> for NamedKey {
    fn from((name, key): (String, Key)) -> Self {
        let mut pb_named_key = NamedKey::new();
        pb_named_key.set_name(name);
        pb_named_key.set_key(key.into());
        pb_named_key
    }
}

impl TryFrom<NamedKey> for (String, Key) {
    type Error = ParsingError;

    fn try_from(mut pb_named_key: NamedKey) -> Result<Self, Self::Error> {
        let key = pb_named_key.take_key().try_into()?;
        let name = pb_named_key.name;
        Ok((name, key))
    }
}

/// Thin wrapper to allow us to implement `From` and `TryFrom` helpers to convert to and from
/// `NamedKeys` and `Vec<NamedKey>`.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct NamedKeyMap(NamedKeys);

impl NamedKeyMap {
    pub fn new(inner: NamedKeys) -> Self {
        Self(inner)
    }

    pub fn into_inner(self) -> NamedKeys {
        self.0
    }
}

impl From<NamedKeyMap> for Vec<NamedKey> {
    fn from(named_key_map: NamedKeyMap) -> Self {
        named_key_map.0.into_iter().map(Into::into).collect()
    }
}

impl TryFrom<Vec<NamedKey>> for NamedKeyMap {
    type Error = ParsingError;

    fn try_from(pb_named_keys: Vec<NamedKey>) -> Result<Self, Self::Error> {
        let mut named_key_map = NamedKeyMap(BTreeMap::new());
        for pb_named_key in pb_named_keys {
            let (name, key) = pb_named_key.try_into()?;
            // TODO - consider returning an error if `insert()` returns `Some`.
            let _ = named_key_map.0.insert(name, key);
        }
        Ok(named_key_map)
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_types::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(string in "\\PC*", key in gens::key_arb()) {
            test_utils::protobuf_round_trip::<(String, Key), NamedKey>((string, key));
        }

        #[test]
        fn map_round_trip(named_keys in gens::named_keys_arb(10)) {
            let named_key_map = NamedKeyMap(named_keys);
            test_utils::protobuf_round_trip::<NamedKeyMap, Vec<NamedKey>>(named_key_map);
        }
    }
}
