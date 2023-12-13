#[derive(Debug)]
pub enum Declaration {
    Null,
    /// Referenced by name in the ABI.
    Ref(String),
    Type(String),
}

#[derive(Debug)]
pub struct EnumVariant {
    pub name: &'static str,
    pub discriminant: u64,
    pub body: Definition,
}

#[derive(Debug)]
pub struct StructField {
    pub name: &'static str,
    pub body: Definition,
}

#[derive(Debug)]
pub enum Definition {
    Type(String),
    Tuple { items: Vec<Definition> },
    Enum { items: Vec<EnumVariant> },
    Struct { items: Vec<StructField> },
}

pub trait CasperABI {
    fn declaration() -> Declaration;
    fn push_recursive(&self, defs: &mut Vec<Definition>) {}
    fn definition() -> Definition;
}

enum CustomError {
    Foo,
    Bar,
    Baz = 123,
}

macro_rules! impl_abi_for_types {
    // Accepts following syntax: impl_abi_for_types(u8, u16, u32, u64, String => "string", f32, f64)
    ($($ty:ty $(=> $name:expr)?,)* ) => {
        $(
            impl_abi_for_types!(@impl $ty $(=> $name)?);
        )*
    };

    (@impl $ty:ty ) => {
       impl_abi_for_types!(@impl $ty => stringify!($ty));
    };

    (@impl $ty:ty => $name:expr ) => {
        impl CasperABI for $ty {
            fn declaration() -> Declaration {
                Declaration::Type($name.into())
            }

            fn definition() -> Definition {
                Definition::Type($name.into())
            }
        }
    };
}

impl CasperABI for () {
    fn declaration() -> Declaration {
        Declaration::Null
    }

    fn definition() -> Definition {
        Definition::Struct { items: Vec::new() }
    }
}

impl_abi_for_types!(
    bool,
    u8, u16, u32, u64,
    i8, i16, i32, i64,
    f32, f64,
    String => "string",
);

impl<T: CasperABI, E: CasperABI> CasperABI for Result<T, E> {
    fn declaration() -> Declaration {
        todo!()
        // Declaration::Type(())
    }

    fn definition() -> Definition {
        Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "Ok",
                    discriminant: 0,
                    body: T::definition(),
                },
                EnumVariant {
                    name: "Err",
                    discriminant: 1,
                    body: E::definition(),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {}
}
