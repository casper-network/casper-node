use crate::{CapnpConvError, CorrespondingCapnp, ReadCapnp, WriteCapnp};
macro_rules! primitive_impl(
    ($typ:ty) => (
        impl CorrespondingCapnp for $typ {
            type Type = crate::converters::generic_owned::Owned<bool>;
        }

        impl<'a> WriteCapnp<'a> for $typ {
            fn write_capnp(&self, builder: &mut <Self::Type as capnp::traits::Owned<'a>>::Builder) {
                builder.builder.set_data_field::<$typ>(0, *self)
            }
        }

        impl<'a> ReadCapnp<'a> for $typ {
            fn read_capnp(
                reader: &<Self::Type as capnp::traits::Owned<'a>>::Reader,
            ) -> Result<Self, CapnpConvError> {
                Ok(reader.reader.get_data_field::<$typ>(0))
            }
        }
        );
    );

primitive_impl!(u8);
primitive_impl!(u16);
primitive_impl!(u32);
primitive_impl!(u64);
primitive_impl!(i8);
primitive_impl!(i16);
primitive_impl!(i32);
primitive_impl!(i64);
primitive_impl!(f32);
primitive_impl!(f64);
