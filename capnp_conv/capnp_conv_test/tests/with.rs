use byteorder::{BigEndian, ByteOrder, ReadBytesExt, WriteBytesExt};
use capnp_conv::{
    capnp_conv, private::CapnpConvWrapper, CapnpConvError, CorrespondingCapnp, FromCapnpBytes,
    ReadCapnp, ToCapnpBytes, WriteCapnp,
};

#[allow(unused)]
mod test_capnp {
    include!(concat!(env!("OUT_DIR"), "/capnp/test_capnp.rs"));
}

#[derive(Debug, PartialEq, Eq, ref_cast::RefCast)]
#[repr(transparent)]
struct Wrapper<T>(T);

// TODO: Automatically generate this with a proc macro
impl<T> From<Wrapper<T>> for CapnpConvWrapper<T> {
    fn from(wrapper: Wrapper<T>) -> Self {
        CapnpConvWrapper::new(wrapper.0)
    }
}

impl CorrespondingCapnp for Wrapper<u128> {
    type Type = crate::test_capnp::custom_u_int128::Owned;
}

impl<'a> WriteCapnp<'a> for Wrapper<u128> {
    fn write_capnp(&self, writer: &mut <Self::Type as capnp::traits::Owned<'a>>::Builder) {
        let mut inner = writer.reborrow().get_inner().unwrap();

        let mut data_bytes = Vec::new();

        data_bytes.write_u128::<BigEndian>(self.0).unwrap();
        let mut cursor = std::io::Cursor::new(AsRef::<[u8]>::as_ref(&data_bytes));

        inner.set_x0(cursor.read_u64::<BigEndian>().unwrap());
        inner.set_x1(cursor.read_u64::<BigEndian>().unwrap());
    }
}

impl<'a> ReadCapnp<'a> for Wrapper<u128> {
    fn read_capnp(
        reader: &<Self::Type as capnp::traits::Owned<'a>>::Reader,
    ) -> Result<Self, CapnpConvError> {
        let inner = reader.get_inner()?;
        let mut vec = Vec::new();
        vec.write_u64::<BigEndian>(inner.get_x0())?;
        vec.write_u64::<BigEndian>(inner.get_x1())?;
        Ok(Wrapper(BigEndian::read_u128(&vec[..])))
    }
}

#[capnp_conv(test_capnp::test_with_struct)]
#[derive(Debug, Clone, PartialEq)]
struct TestWithStruct {
    #[capnp_conv(with = Wrapper<u128>)]
    a: u128,
    b: u64,
}

#[test]
fn capnp_serialize_with_struct() {
    let test_with_struct = TestWithStruct { a: 1u128, b: 2u64 };

    let data = test_with_struct.to_packed_capnp_bytes();
    let test_with_struct2 = TestWithStruct::from_packed_capnp_bytes(&data).unwrap();

    assert_eq!(test_with_struct, test_with_struct2);
}

#[capnp_conv(test_capnp::test_with_enum)]
#[derive(Debug, Clone, PartialEq)]
enum TestWithEnum {
    #[capnp_conv(with = Wrapper<u128>)]
    A(u128),
    B(u64),
    C,
}

#[test]
fn capnp_serialize_with_enum() {
    let test_with_enum = TestWithEnum::A(1u128);

    let data = test_with_enum.to_packed_capnp_bytes();
    let test_with_enum2 = TestWithEnum::from_packed_capnp_bytes(&data).unwrap();

    assert_eq!(test_with_enum, test_with_enum2);
}
