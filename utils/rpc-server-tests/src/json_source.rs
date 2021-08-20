use std::path::{Path, PathBuf};

pub(crate) enum JsonSource<'a> {
    Raw(&'a str),
    File(PathBuf),
}

impl<'a> JsonSource<'a> {
    pub(crate) fn from_raw_string(query: &'a str) -> Self {
        JsonSource::Raw(query)
    }

    pub(crate) fn from_file(path: &Path) -> Self {
        JsonSource::File(path.into())
    }
}
