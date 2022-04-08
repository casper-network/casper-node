/// Creates an array newtype for given length with special access operators already implemented.
#[macro_export]
macro_rules! make_array_newtype {
    ($name:ident, $ty:ty, $len:expr) => {
        pub struct $name([$ty; $len]);

        impl $name {
            pub fn new(source: [$ty; $len]) -> Self {
                $name(source)
            }

            pub fn into_inner(self) -> [$ty; $len] {
                self.0
            }
        }

        impl Clone for $name {
            fn clone(&self) -> $name {
                let &$name(ref dat) = self;
                $name(dat.clone())
            }
        }

        impl Copy for $name {}

        impl PartialEq for $name {
            fn eq(&self, other: &$name) -> bool {
                &self[..] == &other[..]
            }
        }

        impl Eq for $name {}

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &$name) -> Option<core::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &$name) -> core::cmp::Ordering {
                self.0.cmp(&other.0)
            }
        }

        impl core::ops::Index<usize> for $name {
            type Output = $ty;

            fn index(&self, index: usize) -> &$ty {
                let &$name(ref dat) = self;
                &dat[index]
            }
        }

        impl core::ops::Index<core::ops::Range<usize>> for $name {
            type Output = [$ty];

            fn index(&self, index: core::ops::Range<usize>) -> &[$ty] {
                let &$name(ref dat) = self;
                &dat[index]
            }
        }

        impl core::ops::Index<core::ops::RangeTo<usize>> for $name {
            type Output = [$ty];

            fn index(&self, index: core::ops::RangeTo<usize>) -> &[$ty] {
                let &$name(ref dat) = self;
                &dat[index]
            }
        }

        impl core::ops::Index<core::ops::RangeFrom<usize>> for $name {
            type Output = [$ty];

            fn index(&self, index: core::ops::RangeFrom<usize>) -> &[$ty] {
                let &$name(ref dat) = self;
                &dat[index]
            }
        }

        impl core::ops::Index<core::ops::RangeFull> for $name {
            type Output = [$ty];

            fn index(&self, _: core::ops::RangeFull) -> &[$ty] {
                let &$name(ref dat) = self;
                &dat[..]
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
                write!(f, "{}([", stringify!($name))?;
                write!(f, "{:?}", self.0[0])?;
                for item in self.0[1..].iter() {
                    write!(f, ", {:?}", item)?;
                }
                write!(f, "])")
            }
        }

        #[allow(unused_qualifications)]
        impl bytesrepr::ToBytes for $name {
            fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
                self.0.to_bytes()
            }

            fn serialized_length(&self) -> usize {
                self.0.serialized_length()
            }
        }

        impl borsh::BorshSerialize for $name {
            fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
                writer.write(&self.0)?;
                Ok(())
            }
        }

        #[allow(unused_qualifications)]
        impl bytesrepr::FromBytes for $name {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
                let (dat, rem) = <[$ty; $len]>::from_bytes(bytes)?;
                Ok(($name(dat), rem))
            }
        }
    };
}
