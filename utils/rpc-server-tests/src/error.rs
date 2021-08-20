#[derive(Debug)]
pub(crate) enum RpcServerTestError {
    SchemaSyntax(String),
    SchemaIsNotAJson(String),
    UnableToCreateQuery(String),
    Other(String),
}
