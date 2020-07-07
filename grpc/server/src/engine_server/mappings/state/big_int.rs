use std::convert::TryFrom;

use types::{CLValue, U128, U256, U512};

use crate::engine_server::{mappings::ParsingError, state::BigInt};

impl TryFrom<BigInt> for CLValue {
    type Error = ParsingError;

    fn try_from(pb_big_int: BigInt) -> Result<CLValue, Self::Error> {
        let cl_value_result = match pb_big_int.get_bit_width() {
            128 => CLValue::from_t(U128::try_from(pb_big_int)?),
            256 => CLValue::from_t(U256::try_from(pb_big_int)?),
            512 => CLValue::from_t(U512::try_from(pb_big_int)?),
            other => return Err(invalid_bit_width(other)),
        };
        cl_value_result.map_err(|error| ParsingError(format!("{:?}", error)))
    }
}

fn invalid_bit_width(bit_width: u32) -> ParsingError {
    ParsingError(format!(
        "Protobuf BigInt bit width of {} is invalid",
        bit_width
    ))
}

macro_rules! protobuf_conversions_for_uint {
    ($type:ty, $bit_width:literal) => {
        impl From<$type> for BigInt {
            fn from(value: $type) -> Self {
                let mut pb_big_int = BigInt::new();
                pb_big_int.set_value(format!("{}", value));
                pb_big_int.set_bit_width($bit_width);
                pb_big_int
            }
        }

        impl TryFrom<BigInt> for $type {
            type Error = ParsingError;
            fn try_from(pb_big_int: BigInt) -> Result<Self, Self::Error> {
                let value = pb_big_int.get_value();
                match pb_big_int.get_bit_width() {
                    $bit_width => <$type>::from_dec_str(value)
                        .map_err(|error| ParsingError(format!("{:?}", error))),
                    other => Err(invalid_bit_width(other)),
                }
            }
        }
    };
}

protobuf_conversions_for_uint!(U128, 128);
protobuf_conversions_for_uint!(U256, 256);
protobuf_conversions_for_uint!(U512, 512);

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use proptest::proptest;

    use types::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn u128_round_trip(u128 in gens::u128_arb()) {
            test_utils::protobuf_round_trip::<U128, BigInt>(u128);
        }

        #[test]
        fn u256_round_trip(u256 in gens::u256_arb()) {
            test_utils::protobuf_round_trip::<U256, BigInt>(u256);
        }

        #[test]
        fn u512_round_trip(u512 in gens::u512_arb()) {
            test_utils::protobuf_round_trip::<U512, BigInt>(u512);
        }
    }

    fn try_with_bad_value<T>(value: T)
    where
        T: Debug + Into<BigInt> + TryFrom<BigInt>,
        <T as TryFrom<BigInt>>::Error: Debug + Into<ParsingError>,
    {
        let expected_error = ParsingError("InvalidCharacter".to_string());

        let mut invalid_pb_big_int = value.into();
        invalid_pb_big_int.set_value("a".to_string());

        assert_eq!(
            expected_error,
            T::try_from(invalid_pb_big_int.clone()).unwrap_err().into()
        );
        assert_eq!(
            expected_error,
            CLValue::try_from(invalid_pb_big_int).unwrap_err()
        );
    }

    fn try_with_invalid_bit_width<T>(value: T)
    where
        T: Debug + Into<BigInt> + TryFrom<BigInt>,
        <T as TryFrom<BigInt>>::Error: Debug + Into<ParsingError>,
    {
        let bit_width = 127;
        let expected_error = invalid_bit_width(bit_width);

        let mut invalid_pb_big_int = value.into();
        invalid_pb_big_int.set_bit_width(bit_width);

        assert_eq!(
            expected_error,
            T::try_from(invalid_pb_big_int.clone()).unwrap_err().into()
        );
        assert_eq!(
            expected_error,
            CLValue::try_from(invalid_pb_big_int).unwrap_err()
        );
    }

    #[test]
    fn should_fail_to_parse() {
        try_with_bad_value(U128::one());
        try_with_bad_value(U256::one());
        try_with_bad_value(U512::one());

        try_with_invalid_bit_width(U128::one());
        try_with_invalid_bit_width(U256::one());
        try_with_invalid_bit_width(U512::one());
    }
}
