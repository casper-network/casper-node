use crate::{data_source::DataSource, error::RpcServerTestError};

pub(crate) struct TestSuite {
    pub(crate) input: String,
    pub(crate) expected: String, // TODO: Schema
}

impl TestSuite {
    pub(crate) fn new(input: DataSource, expected: DataSource) -> Result<Self, RpcServerTestError> {
        Ok(Self {
            input: input.get()?,
            expected: expected.get()?,
        })
    }
}
