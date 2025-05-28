//! Support library for generated code.

pub trait IntoResult<T, E> {
    fn into_result(self) -> Result<T, E>;
}

pub trait IntoOption<T> {
    fn into_option(self) -> Option<T>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct MyOk;
    #[derive(Debug, PartialEq, Eq)]
    struct MyErr;

    #[derive(Debug, PartialEq, Eq)]

    enum CustomResult {
        Ok(MyOk),
        Err(MyErr),
    }

    #[derive(Debug, PartialEq, Eq)]
    enum CustomOption {
        Some(MyOk),
        None,
    }

    impl IntoResult<MyOk, MyErr> for CustomResult {
        fn into_result(self) -> Result<MyOk, MyErr> {
            match self {
                CustomResult::Ok(ok) => Ok(ok),
                CustomResult::Err(err) => Err(err),
            }
        }
    }

    impl IntoOption<MyOk> for CustomOption {
        fn into_option(self) -> Option<MyOk> {
            match self {
                CustomOption::Some(value) => Some(value),
                CustomOption::None => None,
            }
        }
    }

    #[test]
    fn test_into_result() {
        let ok = CustomResult::Ok(MyOk);
        let err = CustomResult::Err(MyErr);

        assert_eq!(ok.into_result(), Ok(MyOk));
        assert_eq!(err.into_result(), Err(MyErr));
    }

    #[test]
    fn test_into_option() {
        let some = CustomOption::Some(MyOk);
        let none = CustomOption::None;

        assert_eq!(some.into_option(), Some(MyOk));
        assert_eq!(none.into_option(), None);
    }
}
