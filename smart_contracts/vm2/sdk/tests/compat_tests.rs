use casper_contract_sdk::compat;
use casper_types::bytesrepr::ToBytes;
use proptest::prelude::*;

fn convert_to_compat_cl_type(cl_type: casper_types::CLType) -> compat::types::CLType {
    match cl_type {
        casper_types::CLType::Bool => compat::types::CLType::Bool,
        casper_types::CLType::I32 => compat::types::CLType::I32,
        casper_types::CLType::I64 => compat::types::CLType::I64,
        casper_types::CLType::U8 => compat::types::CLType::U8,
        casper_types::CLType::U32 => compat::types::CLType::U32,
        casper_types::CLType::U64 => compat::types::CLType::U64,
        casper_types::CLType::U128 => compat::types::CLType::U128,
        casper_types::CLType::U256 => compat::types::CLType::U256,
        casper_types::CLType::U512 => compat::types::CLType::U512,
        casper_types::CLType::Unit => compat::types::CLType::Unit,
        casper_types::CLType::String => compat::types::CLType::String,
        casper_types::CLType::Key => compat::types::CLType::Key,
        casper_types::CLType::URef => compat::types::CLType::URef,
        casper_types::CLType::PublicKey => compat::types::CLType::PublicKey,
        casper_types::CLType::Option(cltype) => {
            compat::types::CLType::Option(Box::new(convert_to_compat_cl_type(*cltype)))
        }
        casper_types::CLType::List(cltype) => {
            compat::types::CLType::List(Box::new(convert_to_compat_cl_type(*cltype)))
        }
        casper_types::CLType::ByteArray(size) => compat::types::CLType::ByteArray(size),
        casper_types::CLType::Result { ok, err } => compat::types::CLType::Result {
            ok: Box::new(convert_to_compat_cl_type(*ok)),
            err: Box::new(convert_to_compat_cl_type(*err)),
        },
        casper_types::CLType::Map { key, value } => compat::types::CLType::Map {
            key: Box::new(convert_to_compat_cl_type(*key)),
            value: Box::new(convert_to_compat_cl_type(*value)),
        },
        casper_types::CLType::Tuple1([t1]) => {
            compat::types::CLType::Tuple1([Box::new(convert_to_compat_cl_type(*t1))])
        }
        casper_types::CLType::Tuple2([t1, t2]) => compat::types::CLType::Tuple2([
            Box::new(convert_to_compat_cl_type(*t1)),
            Box::new(convert_to_compat_cl_type(*t2)),
        ]),
        casper_types::CLType::Tuple3([t1, t2, t3]) => compat::types::CLType::Tuple3([
            Box::new(convert_to_compat_cl_type(*t1)),
            Box::new(convert_to_compat_cl_type(*t2)),
            Box::new(convert_to_compat_cl_type(*t3)),
        ]),
        casper_types::CLType::Any => compat::types::CLType::Any,
    }
}

fn convert_to_compat_cl_value(cl_value: casper_types::CLValue) -> compat::types::CLValue {
    let (cl_type, bytes) = cl_value.destructure();
    compat::types::CLValue::from_components(
        convert_to_compat_cl_type(cl_type),
        bytes.take_inner().into(),
    )
}

proptest! {
    #[test]
    fn cl_type(cl_type in casper_types::gens::cl_type_arb()) {
        let cl_type_bytes = cl_type.to_bytes().expect("Failed to serialize CLType");
        let compat_cl_type: compat::types::CLType = convert_to_compat_cl_type(cl_type.clone());

        // Serialization produces equal bytes
        let compat_cl_type_bytes = borsh::to_vec(&compat_cl_type)
            .expect("Failed to serialize CompatCLType");
        assert_eq!(cl_type_bytes, compat_cl_type_bytes);

        // Deserialization of compat type produces the same CLType
        let compat_cl_type_from_bytes: compat::types::CLType =
            borsh::from_slice(&compat_cl_type_bytes).expect("Failed to deserialize CompatCLType");
        assert_eq!(compat_cl_type, compat_cl_type_from_bytes);

        let cl_type_from_compat_bytes: casper_types::CLType = casper_types::bytesrepr::deserialize(compat_cl_type_bytes).expect("Failed to deserialize CompatCLType");
        assert_eq!(cl_type, cl_type_from_compat_bytes);

        // Ensure that the original CLType can be serialized and deserialized
        let cl_type_compat_again = convert_to_compat_cl_type(cl_type_from_compat_bytes);
        assert_eq!(compat_cl_type, cl_type_compat_again);
    }

    #[test]
    fn cl_value(cl_value in casper_types::gens::cl_value_arb()) {
        let cl_value_bytes = cl_value.to_bytes().expect("Failed to serialize CLType");
        let compat_cl_value: compat::types::CLValue = convert_to_compat_cl_value(cl_value.clone());

        // Serialization produces equal bytes
        let compat_cl_value_bytes = borsh::to_vec(&compat_cl_value)
            .expect("Failed to serialize CompatCLValue");
        assert_eq!(cl_value_bytes, compat_cl_value_bytes);

        // Deserialization of compat type produces the same CLValue
        let compat_cl_value_from_bytes: compat::types::CLValue =
            borsh::from_slice(&compat_cl_value_bytes).expect("Failed to deserialize CompatCLValue");
        assert_eq!(compat_cl_value, compat_cl_value_from_bytes);

        let cl_value_from_compat_bytes: casper_types::CLValue = casper_types::bytesrepr::deserialize(compat_cl_value_bytes).expect("Failed to deserialize CompatCLValue");
        assert_eq!(cl_value, cl_value_from_compat_bytes);

        // Ensure that the original CLValue can be serialized and deserialized
        let cl_value_compat_again = convert_to_compat_cl_value(cl_value_from_compat_bytes);
        assert_eq!(compat_cl_value, cl_value_compat_again);
    }
}
