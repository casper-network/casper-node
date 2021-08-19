use crate::json_source::JsonSource;

pub(crate) struct TestSuite<'a> {
    pub(crate) input: JsonSource<'a>,
    pub(crate) expected: String, // TODO: Schema
}
