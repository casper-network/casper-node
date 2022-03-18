use crate::CapnpConvError;

pub enum CapnpResult<T> {
    Ok(T),
    Err(CapnpConvError),
}

impl<T> CapnpResult<T> {
    pub fn into_result(self) -> Result<T, CapnpConvError> {
        match self {
            CapnpResult::Ok(t) => Ok(t),
            CapnpResult::Err(e) => Err(e),
        }
    }
}

impl<T> From<T> for CapnpResult<T> {
    fn from(input: T) -> Self {
        CapnpResult::Ok(input)
    }
}

impl<T, E> From<Result<T, E>> for CapnpResult<T>
where
    E: Into<CapnpConvError>,
{
    fn from(input: Result<T, E>) -> Self {
        match input {
            Ok(t) => CapnpResult::Ok(t),
            Err(e) => CapnpResult::Err(e.into()),
        }
    }
}
