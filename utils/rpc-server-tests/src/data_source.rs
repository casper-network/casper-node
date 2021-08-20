use std::{
    convert::TryInto,
    fs,
    path::{Path, PathBuf},
};

use crate::error::RpcServerTestError;

#[allow(dead_code)]
pub(crate) enum DataSource<'a> {
    Raw(&'a str),
    File(PathBuf),
}

impl<'a> DataSource<'a> {
    #[allow(dead_code)]
    pub(crate) fn from_raw_string(query: &'a str) -> Self {
        DataSource::Raw(query)
    }

    pub(crate) fn from_file(path: &Path) -> Self {
        DataSource::File(path.into())
    }
}

impl TryInto<String> for DataSource<'_> {
    type Error = RpcServerTestError;

    fn try_into(self) -> Result<String, Self::Error> {
        Ok(match self {
            DataSource::Raw(query) => query.to_string(),
            DataSource::File(path) => fs::read_to_string(path)
                .map_err(|err| RpcServerTestError::ErrorInDataSource(err.to_string()))?,
        })
    }
}
