mod in_mem_error;

pub(crate) use in_mem_error::InMemError;
pub(crate) type InMemResult<T> = Result<T, InMemError>;
