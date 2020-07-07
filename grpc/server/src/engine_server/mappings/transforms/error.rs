use std::convert::TryFrom;

use node::components::contract_runtime::shared::{transform, TypeMismatch};

use crate::engine_server::{
    mappings::ParsingError,
    transforms::{self, TransformFailure, TransformFailure_oneof_failure_instance},
};

impl From<TypeMismatch> for transforms::TypeMismatch {
    fn from(type_mismatch: TypeMismatch) -> transforms::TypeMismatch {
        let mut pb_type_mismatch = transforms::TypeMismatch::new();
        pb_type_mismatch.set_expected(type_mismatch.expected);
        pb_type_mismatch.set_found(type_mismatch.found);
        pb_type_mismatch
    }
}

impl From<transform::Error> for TransformFailure {
    fn from(error: transform::Error) -> Self {
        let mut pb_transform_failure = TransformFailure::new();
        match error {
            transform::Error::TypeMismatch(type_mismatch) => {
                pb_transform_failure.set_type_mismatch(type_mismatch.into())
            }
            transform::Error::Serialization(_error) => panic!("don't break the API"),
        }
        pb_transform_failure
    }
}

impl TryFrom<TransformFailure> for transform::Error {
    type Error = ParsingError;

    fn try_from(pb_transform_failure: TransformFailure) -> Result<transform::Error, ParsingError> {
        let pb_transform_failure = pb_transform_failure
            .failure_instance
            .ok_or_else(|| ParsingError::from("Unable to parse Protobuf TransformFailure"))?;
        match pb_transform_failure {
            TransformFailure_oneof_failure_instance::type_mismatch(transforms::TypeMismatch {
                expected,
                found,
                ..
            }) => {
                let type_mismatch = TypeMismatch { expected, found };
                Ok(transform::Error::TypeMismatch(type_mismatch))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_server::mappings::test_utils;

    #[test]
    fn round_trip() {
        let error = transform::Error::TypeMismatch(TypeMismatch::new(
            "expected".to_string(),
            "found".to_string(),
        ));
        test_utils::protobuf_round_trip::<transform::Error, TransformFailure>(error);
    }
}
