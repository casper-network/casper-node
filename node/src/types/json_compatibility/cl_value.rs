//! This file provides types to allow conversion from a `casper_types::CLValue` into a similar type
//! which can be serialized to a valid JSON representation.

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{CLType, CLValue as ExecutionEngineCLValue};

/// Representation of a `casper_types::CLValue`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
pub struct CLValue {
    /// The type of the `CLValue`.
    #[data_size(skip)]
    pub cl_type: CLType,
    /// The hex-encoded bytes of the `CLValue`.
    pub bytes: String,
}

impl From<&ExecutionEngineCLValue> for CLValue {
    fn from(ee_cl_value: &ExecutionEngineCLValue) -> Self {
        CLValue {
            cl_type: ee_cl_value.cl_type().clone(),
            bytes: hex::encode(ee_cl_value.inner_bytes()),
        }
    }
}
