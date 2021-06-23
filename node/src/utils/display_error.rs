//! Error formatting workaround.
//!
//! This module can be removed once/if the tracing issue
//! https://github.com/tokio-rs/tracing/issues/1308 has been resolved, which adds a special syntax
//! for this case and the known issue https://github.com/tokio-rs/tracing/issues/1308 has been
//! fixed, which cuts traces short after the first cause.
//!
//! In the meantime, the `display_error` function should be used to format errors in log messages.

use std::{
    error,
    fmt::{self, Display, Formatter},
};

use tracing::field;

/// Wraps an error to ensure it gets properly captured by tracing.
pub(crate) fn display_error<'a, T>(err: &'a T) -> field::DisplayValue<ErrFormatter<'a, T>>
where
    T: error::Error + 'a,
{
    field::display(ErrFormatter(err))
}

/// An error formatter.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ErrFormatter<'a, T>(pub &'a T);

impl<'a, T> Display for ErrFormatter<'a, T>
where
    T: error::Error,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut opt_source: Option<&(dyn error::Error)> = Some(self.0);

        while let Some(source) = opt_source {
            write!(f, "{}", source)?;
            opt_source = source.source();

            if opt_source.is_some() {
                f.write_str(": ")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use thiserror::Error;

    use super::ErrFormatter;

    #[derive(Debug, Error)]
    #[error("this is baz")]
    struct Baz;

    #[derive(Debug, Error)]
    #[error("this is bar")]
    struct Bar(#[source] Baz);

    #[derive(Debug, Error)]
    enum MyError {
        #[error("this is foo")]
        Foo {
            #[source]
            bar: Bar,
        },
    }

    #[test]
    fn test_formatter_formats_single() {
        let single = Baz;

        assert_eq!(ErrFormatter(&single).to_string().as_str(), "this is baz");
    }

    #[test]
    fn test_formatter_formats_nested() {
        let nested = MyError::Foo { bar: Bar(Baz) };

        assert_eq!(
            ErrFormatter(&nested).to_string().as_str(),
            "this is foo: this is bar: this is baz"
        );
    }
}
