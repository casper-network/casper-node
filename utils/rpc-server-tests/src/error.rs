#[derive(Debug)]
pub(crate) enum RpcServerTestError {
    SchemaSyntax(String),
    SchemaIsNotAJson(String),
    ErrorInDataSource(String),
    Other(String),
}
