#[derive(Copy, Clone)]
pub struct Owned<T> {
    _phantom: core::marker::PhantomData<T>,
}

impl<'a, T> capnp::traits::Owned<'a> for Owned<T> {
    type Reader = Reader<'a>;
    type Builder = Builder<'a, T>;
}

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub reader: capnp::private::layout::StructReader<'a>,
}

impl<'a> capnp::traits::FromStructReader<'a> for Reader<'a> {
    fn new(reader: capnp::private::layout::StructReader<'a>) -> Reader<'a> {
        Reader { reader }
    }
}

impl<'a> capnp::traits::FromPointerReader<'a> for Reader<'a> {
    fn get_from_pointer(
        reader: &::capnp::private::layout::PointerReader<'a>,
        default: core::option::Option<&'a [capnp::Word]>,
    ) -> capnp::Result<Reader<'a>> {
        core::result::Result::Ok(capnp::traits::FromStructReader::new(
            reader.get_struct(default)?,
        ))
    }
}

pub struct Builder<'a, T> {
    pub builder: capnp::private::layout::StructBuilder<'a>,
    _phantom: core::marker::PhantomData<T>,
}

impl<'a, T> capnp::traits::FromStructBuilder<'a> for Builder<'a, T> {
    fn new(builder: capnp::private::layout::StructBuilder<'a>) -> Self {
        Builder {
            builder,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<'a, T> capnp::traits::FromPointerBuilder<'a> for Builder<'a, T>
where
    T: Sized,
{
    fn init_pointer(builder: capnp::private::layout::PointerBuilder<'a>, _size: u32) -> Self {
        capnp::traits::FromStructBuilder::new(builder.init_struct(
            capnp::private::layout::StructSize {
                data: core::mem::size_of::<T>() as u16,
                pointers: 0,
            },
        ))
    }
    fn get_from_pointer(
        builder: capnp::private::layout::PointerBuilder<'a>,
        default: core::option::Option<&'a [capnp::Word]>,
    ) -> capnp::Result<Self> {
        core::result::Result::Ok(capnp::traits::FromStructBuilder::new(builder.get_struct(
            capnp::private::layout::StructSize {
                data: core::mem::size_of::<T>() as u16,
                pointers: 0,
            },
            default,
        )?))
    }
}

impl<'a> capnp::traits::SetPointerBuilder for Reader<'a> {
    fn set_pointer_builder<'b>(
        pointer: capnp::private::layout::PointerBuilder<'b>,
        value: Reader<'a>,
        canonicalize: bool,
    ) -> capnp::Result<()> {
        pointer.set_struct(&value.reader, canonicalize)
    }
}
