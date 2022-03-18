use crate::{converters::generic_owned, CapnpConvError, CorrespondingCapnp, ReadCapnp, WriteCapnp};

impl CorrespondingCapnp for () {
    type Type = generic_owned::Owned<()>;
}

impl<'a> WriteCapnp<'a> for () {
    fn write_capnp(&self, _writer: &mut <Self::Type as capnp::traits::Owned<'a>>::Builder) {}
}

impl<'a> ReadCapnp<'a> for () {
    fn read_capnp(
        _reader: &<Self::Type as capnp::traits::Owned<'a>>::Reader,
    ) -> Result<Self, CapnpConvError> {
        Ok(())
    }
}
