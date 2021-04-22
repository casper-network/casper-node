use std::collections::BTreeMap;

use casper_types::{
    bytesrepr::{self, ToBytes},
    CLValue, Key, U512,
};
use serde::Deserialize;

/// Representation of supported input value.
#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "value")]
pub enum Input {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    String(String),
    Bool(bool),
    U512(U512),
    CLValue(CLValue),
    Key(Key),
}

impl ToBytes for Input {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            Input::U8(value) => value.to_bytes(),
            Input::U16(value) => value.to_bytes(),
            Input::U32(value) => value.to_bytes(),
            Input::U64(value) => value.to_bytes(),
            Input::String(value) => value.to_bytes(),
            Input::Bool(value) => value.to_bytes(),
            Input::U512(value) => value.to_bytes(),
            Input::CLValue(value) => value.to_bytes(),
            Input::Key(value) => value.to_bytes(),
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            Input::U8(value) => value.serialized_length(),
            Input::U16(value) => value.serialized_length(),
            Input::U32(value) => value.serialized_length(),
            Input::U64(value) => value.serialized_length(),
            Input::String(value) => value.serialized_length(),
            Input::Bool(value) => value.serialized_length(),
            Input::U512(value) => value.serialized_length(),
            Input::CLValue(value) => value.serialized_length(),
            Input::Key(value) => value.serialized_length(),
        }
    }
}

/// Test case defines a list of inputs and an output.
#[derive(Deserialize, Debug)]
pub struct ABITestCase {
    input: Vec<Input>,
    #[serde(deserialize_with = "hex::deserialize")]
    output: Vec<u8>,
}

impl ABITestCase {
    pub fn input(&self) -> &[Input] {
        &self.input
    }

    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// This gets executed for each test case.
    pub fn run_test(&self) {
        let serialized_length = self.serialized_length();
        let serialized_data = self.to_bytes().expect("should serialize");

        // Serialized data should match the output
        assert_eq!(
            serialized_data,
            self.output(),
            "{} != {}",
            base16::encode_lower(&serialized_data),
            base16::encode_lower(&self.output())
        );

        // Output from serialized_length should match the output data length
        assert_eq!(serialized_length, self.output().len(),);
    }
}

impl ToBytes for ABITestCase {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut res = Vec::with_capacity(self.serialized_length());

        for input in &self.input {
            res.append(&mut input.to_bytes()?);
        }

        Ok(res)
    }

    fn serialized_length(&self) -> usize {
        self.input.iter().map(ToBytes::serialized_length).sum()
    }
}

/// A fixture consists of multiple test cases.
#[derive(Deserialize, Debug)]
pub struct ABIFixture(BTreeMap<String, ABITestCase>);

impl ABIFixture {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(self) -> BTreeMap<String, ABITestCase> {
        self.0
    }
}
