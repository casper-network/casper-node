use crate::{converters::generic_owned, CapnpConvError, CorrespondingCapnp, ReadCapnp, WriteCapnp};

impl CorrespondingCapnp for bool {
    type Type = generic_owned::Owned<bool>;
}

impl<'a> WriteCapnp<'a> for bool {
    fn write_capnp(&self, builder: &mut <Self::Type as capnp::traits::Owned<'a>>::Builder) {
        builder.builder.set_bool_field(0, *self)
    }
}

impl<'a> ReadCapnp<'a> for bool {
    fn read_capnp(
        reader: &<Self::Type as capnp::traits::Owned<'a>>::Reader,
    ) -> Result<Self, CapnpConvError> {
        Ok(reader.reader.get_bool_field(0))
    }
}
