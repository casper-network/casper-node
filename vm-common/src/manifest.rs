use std::io::{Error, ErrorKind};

use borsh::{BorshDeserialize, BorshSerialize};
use casper_sdk_sys::Fptr;

use crate::leb128::LEB128U;

const CURRENT_MANIFEST_VERSION: u32 = 1;

pub struct EntryPoint(pub(crate) casper_sdk_sys::EntryPoint);

impl EntryPoint {
    pub fn into_inner(self) -> casper_sdk_sys::EntryPoint {
        self.0
    }
}

impl From<casper_sdk_sys::EntryPoint> for EntryPoint {
    fn from(entry_point: casper_sdk_sys::EntryPoint) -> Self {
        Self(entry_point)
    }
}

impl BorshSerialize for EntryPoint {
    fn serialize<W: std::io::prelude::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let casper_sdk_sys::EntryPoint {
            selector,
            fptr,
            flags,
        } = self.0;
        selector.serialize(writer)?;

        let fptr_integer = {
            // Be aware that this is not portable across all platforms and makes sense only under wasm32 where each function pointer is guaranteed to be part of a wasm table entry.
            let fptr_integer = fptr as usize;
            LEB128U::new(fptr_integer as u64)
        };

        fptr_integer.serialize(writer)?;
        flags.serialize(writer)?;
        Ok(())
    }
}

impl BorshDeserialize for EntryPoint {
    fn deserialize_reader<R: std::io::prelude::Read>(reader: &mut R) -> std::io::Result<Self> {
        let selector = u32::deserialize_reader(reader)?;
        let fptr_integer = LEB128U::deserialize_reader(reader)?;
        let fptr = {
            let fptr_integer: u64 = fptr_integer.into_inner();
            let fptr = fptr_integer as *const ();
            let fptr = unsafe { std::mem::transmute(fptr) };
            fptr
        };
        let flags = u32::deserialize_reader(reader)?;
        Ok(Self(casper_sdk_sys::EntryPoint {
            selector,
            fptr,
            flags,
        }))
    }
}

pub struct Manifest(Vec<EntryPoint>);

impl Manifest {
    pub fn entry_points(&self) -> &[EntryPoint] {
        &self.0
    }
}

impl From<casper_sdk_sys::Manifest> for Manifest {
    fn from(manifest: casper_sdk_sys::Manifest) -> Self {
        let slice = unsafe {
            std::slice::from_raw_parts(manifest.entry_points, manifest.entry_points_size)
        };
        let entry_points = slice
            .into_iter()
            .map(|entry_point| EntryPoint::from(*entry_point))
            .collect();
        Self(entry_points)
    }
}

impl BorshDeserialize for Manifest {
    fn deserialize_reader<R: std::io::prelude::Read>(reader: &mut R) -> std::io::Result<Self> {
        let version: u32 = u32::deserialize_reader(reader)?;
        if version != CURRENT_MANIFEST_VERSION {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("Unsupported manifest version: {}", version),
            ));
        }
        let entrypoints = BorshDeserialize::deserialize_reader(reader)?;
        Ok(Manifest(entrypoints))
    }
}

impl BorshSerialize for Manifest {
    fn serialize<W: std::io::prelude::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        CURRENT_MANIFEST_VERSION.serialize(writer)?;
        self.0.serialize(writer)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        pub static GLOBAL_FLAG: RefCell<usize> = RefCell::new(0);
    }

    extern "C" fn dummy() {
        GLOBAL_FLAG.with_borrow_mut(|v| {
            *v += 1;
        });
    }
    const ENTRYPOINTS: [casper_sdk_sys::EntryPoint; 1] = [casper_sdk_sys::EntryPoint {
        selector: 123,
        fptr: dummy,
        flags: 0x0b4dc0de,
    }];
    const MANIFEST: casper_sdk_sys::Manifest = casper_sdk_sys::Manifest {
        entry_points: ENTRYPOINTS.as_ptr(),
        entry_points_size: ENTRYPOINTS.len(),
    };

    #[test]
    fn foo() {
        let bytes = {
            let entrypoint = Manifest::from(MANIFEST);
            let bytes = borsh::to_vec(&entrypoint).unwrap();
            bytes
        };
        let manifest_from_bytes = borsh::from_slice::<Manifest>(&bytes).unwrap();
        assert_eq!(manifest_from_bytes.entry_points().len(), 1);
        let inner = manifest_from_bytes.entry_points().get(0).unwrap();
        assert_eq!(inner.0.selector, 123);
        assert_eq!(inner.0.flags, 0xb4dc0de);
        let before = GLOBAL_FLAG.with_borrow(|v| *v);
        assert!(inner.0.fptr == dummy);
        (inner.0.fptr)();
        let after = GLOBAL_FLAG.with_borrow(|v| *v);
        assert_eq!(after.checked_sub(before), Some(1));
    }
}
