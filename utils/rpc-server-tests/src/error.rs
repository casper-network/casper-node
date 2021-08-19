#[derive(Debug)]
pub(crate) enum RpcServerTestError {
    SchemaSyntax(String),
    SchemaIsNotAJson(String),
    Other(String),
}
