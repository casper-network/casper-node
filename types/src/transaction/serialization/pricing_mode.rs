use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
};
use crate::{
    bytesrepr::{self, Bytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, PricingMode,
};
use alloc::{collections::BTreeMap, vec::Vec};

const CLASSIC_TAG: u8 = 0;
const FIXED_TAG: u8 = 1;
const RESERVED_TAG: u8 = 2;

const TAG_FIELD_META_INDEX: u16 = 1;
const PAYMENT_AMOUNT_FIELD_META_INDEX: u16 = 2;
const CLASSIC_GAS_PRICE_TOLERANCE_FIELD_META_INDEX: u16 = 3;
const STANDARD_PAYMENT_FIELD_META_INDEX: u16 = 4;
const RECEIPT_FIELD_META_INDEX: u16 = 5;
const GAS_PRICE_TOLERANCE_FIELD_META_INDEX: u16 = 6;

pub fn classic_serialized_length(
    payment_amount: &u64,
    gas_price_tolerance: &u8,
    standard_payment: &bool,
) -> usize {
    serialized_length_for_field_sizes(vec![
        U8_SERIALIZED_LENGTH,
        payment_amount.serialized_length(),
        gas_price_tolerance.serialized_length(),
        standard_payment.serialized_length(),
    ])
}

pub fn serialize_classic_pricing_mode(
    payment_amount: &u64,
    gas_price_tolerance: &u8,
    standard_payment: &bool,
) -> Result<Vec<u8>, crate::bytesrepr::Error> {
    let mut fields = BTreeMap::new();
    let tag_field_bytes = CLASSIC_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));
    let payment_amount_bytes = payment_amount.to_bytes()?;
    fields.insert(
        PAYMENT_AMOUNT_FIELD_META_INDEX,
        Bytes::from(payment_amount_bytes),
    );
    let gas_price_tolerance_bytes = gas_price_tolerance.to_bytes()?;
    fields.insert(
        CLASSIC_GAS_PRICE_TOLERANCE_FIELD_META_INDEX,
        Bytes::from(gas_price_tolerance_bytes),
    );
    let standard_payment_bytes = standard_payment.to_bytes()?;
    fields.insert(
        STANDARD_PAYMENT_FIELD_META_INDEX,
        Bytes::from(standard_payment_bytes),
    );
    serialize_fields_map(fields)
}

pub fn reserved_serialized_length(receipt: &Digest) -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH, receipt.serialized_length()])
}

pub fn serialize_reserved_pricing_mode(
    receipt: &Digest,
) -> Result<Vec<u8>, crate::bytesrepr::Error> {
    let mut fields = BTreeMap::new();
    let tag_field_bytes = RESERVED_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));
    let receipt_bytes = receipt.to_bytes()?;
    fields.insert(RECEIPT_FIELD_META_INDEX, Bytes::from(receipt_bytes));
    serialize_fields_map(fields)
}

pub fn fixed_serialized_length(gas_price_tolerance: &u8) -> usize {
    serialized_length_for_field_sizes(vec![
        U8_SERIALIZED_LENGTH,
        gas_price_tolerance.serialized_length(),
    ])
}

pub fn serialize_fixed_pricing_mode(
    gas_price_tolerance: &u8,
) -> Result<Vec<u8>, crate::bytesrepr::Error> {
    let mut fields = BTreeMap::new();
    let tag_field_bytes = FIXED_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));
    let gas_price_tolerance_bytes = gas_price_tolerance.to_bytes()?;
    fields.insert(
        GAS_PRICE_TOLERANCE_FIELD_META_INDEX,
        Bytes::from(gas_price_tolerance_bytes),
    );
    serialize_fields_map(fields)
}

pub fn deserialize_pricing_mode(bytes: &[u8]) -> Result<(PricingMode, &[u8]), bytesrepr::Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
    let tag = consume_field::<u8>(&mut data_map, TAG_FIELD_META_INDEX)?;
    match tag {
        CLASSIC_TAG => {
            let payment_amount =
                consume_field::<u64>(&mut data_map, PAYMENT_AMOUNT_FIELD_META_INDEX)?;
            let gas_price_tolerance =
                consume_field::<u8>(&mut data_map, CLASSIC_GAS_PRICE_TOLERANCE_FIELD_META_INDEX)?;
            let standard_payment =
                consume_field::<bool>(&mut data_map, STANDARD_PAYMENT_FIELD_META_INDEX)?;
            Ok((
                PricingMode::Classic {
                    payment_amount,
                    gas_price_tolerance,
                    standard_payment,
                },
                remainder,
            ))
        }
        FIXED_TAG => {
            let gas_price_tolerance =
                consume_field::<u8>(&mut data_map, GAS_PRICE_TOLERANCE_FIELD_META_INDEX)?;
            Ok((
                PricingMode::Fixed {
                    gas_price_tolerance,
                },
                remainder,
            ))
        }
        RESERVED_TAG => {
            let receipt = consume_field::<Digest>(&mut data_map, RECEIPT_FIELD_META_INDEX)?;
            Ok((PricingMode::Reserved { receipt }, remainder))
        }
        _ => Err(bytesrepr::Error::Formatting),
    }
}
