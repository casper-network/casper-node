#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::vec::Vec;

mod converters;
pub mod private;

pub use capnp_conv_derive::capnp_conv;

// It is helpful to have a wildcard error like Box<dyn std::error::Error>
// This is because there are many ways that deserialization can fail.
// However, anyhow is used because std::error::Error is not compatible with no_std.
// See https://github.com/rust-lang/rust/issues/48331#issuecomment-576942330
#[derive(Debug)]
#[repr(transparent)]
pub struct CapnpConvError(anyhow::Error);

impl CapnpConvError {
    pub fn new(error: anyhow::Error) -> Self {
        CapnpConvError(error)
    }
}

#[macro_export]
macro_rules! capnp_conv_error {
    ($msg:literal $(,)?) => ({
        $crate::CapnpConvError::new(::anyhow::anyhow!($msg))
    });
    ($err:expr $(,)?) => ({
        $crate::CapnpConvError::new(::anyhow::anyhow!($err))
    });
    ($fmt:expr, $($arg:tt)*) => {
        $crate::CapnpConvError::new(::anyhow::anyhow!($fmt, $($arg)*))
    };
}

#[cfg(feature = "std")]
impl<E> From<E> for CapnpConvError
where
    E: std::error::Error + Send + Sync + 'static,
{
    #[cold]
    fn from(error: E) -> Self {
        capnp_conv_error!(error)
    }
}

#[cfg(not(feature = "std"))]
impl From<capnp::Error> for CapnpConvError {
    fn from(capnp_conv_error: capnp::Error) -> Self {
        capnp_conv_error!(capnp_conv_error)
    }
}

#[cfg(not(feature = "std"))]
impl From<capnp::NotInSchema> for CapnpConvError {
    fn from(not_in_schema_error: capnp::NotInSchema) -> Self {
        capnp_conv_error!(not_in_schema_error)
    }
}

/// Corresponding Capnp type used for conversion operations.
pub trait CorrespondingCapnp {
    /// The corresponding Capnp type.
    type Type: for<'c> capnp::traits::Owned<'c>;
}

/// Convert Rust struct to Capnp.
pub trait WriteCapnp<'a>: CorrespondingCapnp {
    /// Converts a Rust struct to corresponding Capnp struct. This should not fail.
    fn write_capnp(&self, builder: &mut <Self::Type as capnp::traits::Owned<'a>>::Builder);
}

/// Convert Capnp struct to Rust.
pub trait ReadCapnp<'a>: CorrespondingCapnp + Sized {
    /// Converts a Capnp struct to corresponding Rust struct.     
    fn read_capnp(
        reader: &<Self::Type as capnp::traits::Owned<'a>>::Reader,
    ) -> Result<Self, CapnpConvError>;
}

pub trait ToCapnpBytes {
    /// Serialize a Rust struct into bytes using Capnp
    fn to_packed_capnp_bytes(&self) -> Vec<u8>;
}

pub trait FromCapnpBytes: Sized {
    /// Deserialize a Rust struct from bytes using Capnp
    fn from_packed_capnp_bytes(bytes: &[u8]) -> Result<Self, CapnpConvError>;
}

impl<T> ToCapnpBytes for T
where
    T: for<'a> WriteCapnp<'a>,
{
    fn to_packed_capnp_bytes(&self) -> Vec<u8> {
        let mut builder = capnp::message::Builder::new_default();

        // A trick to avoid borrow checker issues:
        {
            let mut struct_builder =
                builder.init_root::<<T::Type as capnp::traits::Owned>::Builder>();
            self.write_capnp(&mut struct_builder);
        }

        let mut data = Vec::new();
        // Should never really fail:
        capnp::serialize_packed::write_message(&mut data, &builder).unwrap();
        data
    }
}

impl<T> FromCapnpBytes for T
where
    T: for<'a> ReadCapnp<'a>,
{
    fn from_packed_capnp_bytes(bytes: &[u8]) -> Result<Self, CapnpConvError> {
        let reader =
            capnp::serialize_packed::read_message(bytes, capnp::message::ReaderOptions::new())?;
        let struct_reader = reader.get_root::<<T::Type as capnp::traits::Owned>::Reader>()?;

        Self::read_capnp(&struct_reader)
    }
}
