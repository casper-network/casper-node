use std::convert::{TryFrom, TryInto};

use types::CLValue;

use crate::engine_server::{mappings::ParsingError, state};

impl From<CLValue> for state::CLValue {
    fn from(cl_value: CLValue) -> Self {
        let (cl_type, bytes) = cl_value.destructure();

        let mut pb_value = state::CLValue::new();
        pb_value.set_cl_type(cl_type.into());
        pb_value.set_serialized_value(bytes);

        pb_value
    }
}

impl TryFrom<state::CLValue> for CLValue {
    type Error = ParsingError;

    fn try_from(mut pb_value: state::CLValue) -> Result<Self, Self::Error> {
        let cl_type = pb_value.take_cl_type().try_into()?;
        Ok(CLValue::from_components(cl_type, pb_value.serialized_value))
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use types::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(cl_value in gens::cl_value_arb()) {
            test_utils::protobuf_round_trip::<CLValue, state::CLValue>(cl_value);
        }
    }
}
