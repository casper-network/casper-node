#[derive(Debug)]
pub(crate) enum RpcServerTestError {
    SchemaSyntax(String),
    SchemaIsNotAJson(String),
    ErrorInDataSource(String),
    ExpectedCodeFileError(String),
    IncorrectExpectedCode(),
    CantExtractErrorCode(),
    Other(String),
}
