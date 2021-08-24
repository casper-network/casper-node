use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr::{self, ToBytes},
    CLValue, ExecutionResult, Key, StoredValue, Transform, U512,
};

use crate::test_case::{Error, TestCase};

/// Representation of supported input value.
#[derive(Serialize, Deserialize, Debug, From)]
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
    Transform(Transform),
    StoredValue(StoredValue),
    ExecutionResult(ExecutionResult),
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
            Input::Transform(value) => value.to_bytes(),
            Input::StoredValue(value) => value.to_bytes(),
            Input::ExecutionResult(value) => value.to_bytes(),
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
            Input::Transform(value) => value.serialized_length(),
            Input::StoredValue(value) => value.serialized_length(),
            Input::ExecutionResult(value) => value.serialized_length(),
        }
    }
}

/// Test case defines a list of inputs and an output.
#[derive(Serialize, Deserialize, Debug)]
pub struct ABITestCase {
    input: Vec<Input>,
    #[serde(
        deserialize_with = "hex::deserialize",
        serialize_with = "hex::serialize"
    )]
    output: Vec<u8>,
}

impl ABITestCase {
    pub fn from_inputs(inputs: Vec<Input>) -> Result<ABITestCase, bytesrepr::Error> {
        // This is manually going through each input passed as we can't use `ToBytes for Vec<T>` as
        // the `output` would be a serialized collection.
        let mut truth = Vec::new();
        for input in &inputs {
            // Input::to_bytes uses static dispatch to call into each raw value impl.
            let mut generated_truth = input.to_bytes()?;
            truth.append(&mut generated_truth);
        }

        Ok(ABITestCase {
            input: inputs,
            output: truth,
        })
    }

    pub fn input(&self) -> &[Input] {
        &self.input
    }

    pub fn output(&self) -> &[u8] {
        &self.output
    }
}

impl TestCase for ABITestCase {
    /// Compares input to output.
    ///
    /// This gets executed for each test case.
    fn run_test(&self) -> Result<(), Error> {
        let serialized_length = self.serialized_length();
        let serialized_data = self.to_bytes()?;

        // Serialized data should match the output
        if serialized_data != self.output() {
            return Err(Error::DataMismatch {
                actual: serialized_data,
                expected: self.output().to_vec(),
            });
        }

        // Output from serialized_length should match the output data length
        if serialized_length != self.output().len() {
            return Err(Error::LengthMismatch {
                expected: serialized_length,
                actual: self.output().len(),
            });
        }

        Ok(())
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
#[derive(Serialize, Deserialize, Debug, From)]
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
