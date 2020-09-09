use std::convert::{TryFrom, TryInto};

use casper_execution_engine::shared::{
    stored_value::StoredValue,
    transform::{Error as TransformError, Transform},
};
use casper_types::{CLType, CLValue, U128, U256, U512};

use crate::engine_server::{
    mappings::{state::NamedKeyMap, ParsingError},
    state::NamedKey,
    transforms::{self, Transform_oneof_transform_instance},
};

impl From<Transform> for transforms::Transform {
    fn from(transform: Transform) -> Self {
        let mut pb_transform = transforms::Transform::new();
        match transform {
            Transform::Identity => {
                pb_transform.set_identity(Default::default());
            }
            Transform::AddInt32(i) => {
                pb_transform.mut_add_i32().set_value(i);
            }
            Transform::AddUInt64(u) => {
                pb_transform.mut_add_u64().set_value(u);
            }
            Transform::Write(value) => {
                pb_transform.mut_write().set_value(value.into());
            }
            Transform::AddKeys(keys_map) => {
                let pb_named_keys: Vec<NamedKey> = NamedKeyMap::new(keys_map).into();
                pb_transform.mut_add_keys().set_value(pb_named_keys.into());
            }
            Transform::Failure(transform_error) => pb_transform.set_failure(transform_error.into()),
            Transform::AddUInt128(uint128) => {
                pb_transform.mut_add_big_int().set_value(uint128.into());
            }
            Transform::AddUInt256(uint256) => {
                pb_transform.mut_add_big_int().set_value(uint256.into());
            }
            Transform::AddUInt512(uint512) => {
                pb_transform.mut_add_big_int().set_value(uint512.into());
            }
        };
        pb_transform
    }
}

impl TryFrom<transforms::Transform> for Transform {
    type Error = ParsingError;

    fn try_from(pb_transform: transforms::Transform) -> Result<Self, Self::Error> {
        let pb_transform = pb_transform
            .transform_instance
            .ok_or_else(|| ParsingError::from("Unable to parse Protobuf Transform"))?;
        let transform = match pb_transform {
            Transform_oneof_transform_instance::identity(_) => Transform::Identity,
            Transform_oneof_transform_instance::add_keys(pb_add_keys) => {
                let named_keys_map: NamedKeyMap = pb_add_keys.value.into_vec().try_into()?;
                named_keys_map.into_inner().into()
            }
            Transform_oneof_transform_instance::add_i32(pb_add_int32) => pb_add_int32.value.into(),
            Transform_oneof_transform_instance::add_u64(pb_add_u64) => pb_add_u64.value.into(),
            Transform_oneof_transform_instance::add_big_int(mut pb_big_int) => {
                let cl_value: CLValue = pb_big_int.take_value().try_into()?;
                match cl_value.cl_type() {
                    CLType::U128 => {
                        let u128: U128 = cl_value
                            .into_t()
                            .map_err(|error| ParsingError(format!("{:?}", error)))?;
                        u128.into()
                    }
                    CLType::U256 => {
                        let u256: U256 = cl_value
                            .into_t()
                            .map_err(|error| ParsingError(format!("{:?}", error)))?;
                        u256.into()
                    }
                    CLType::U512 => {
                        let u512: U512 = cl_value
                            .into_t()
                            .map_err(|error| ParsingError(format!("{:?}", error)))?;
                        u512.into()
                    }
                    other => {
                        return Err(ParsingError(format!(
                            "Protobuf BigInt was turned into a non-uint Value type: {:?}",
                            other
                        )));
                    }
                }
            }
            Transform_oneof_transform_instance::write(mut pb_write) => {
                let value = StoredValue::try_from(pb_write.take_value())?;
                Transform::Write(value)
            }
            Transform_oneof_transform_instance::failure(pb_failure) => {
                let error = TransformError::try_from(pb_failure)?;
                Transform::Failure(error)
            }
        };
        Ok(transform)
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::transform::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(transform in gens::transform_arb()) {
            test_utils::protobuf_round_trip::<Transform, transforms::Transform>(transform);
        }
    }
}
