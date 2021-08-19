use std::path::Path;

pub(crate) enum JsonSource<'a> {
    Raw(&'a str),
    File(&'a dyn AsRef<Path>),
}

impl<'a> JsonSource<'a> {
    pub(crate) fn from_raw_string(query: &'a str) -> Self {
        JsonSource::Raw(query)
    }
}
