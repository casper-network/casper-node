use std::{io, marker::PhantomData};

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    abi::{CasperABI, Declaration, Definition},
    host, reserve_vec_space,
    storage::Keyspace,
};

#[derive(Debug)]
pub struct Field<T> {
    name: &'static str,
    key_space: u64, // KeyTag
    _marker: PhantomData<T>,
}

impl<T: CasperABI> CasperABI for Field<T> {
    #[inline]
    fn definition() -> Definition {
        T::definition()
    }
}

impl<T: BorshSerialize> Field<T> {
    pub fn new(name: &'static str, key_space: u64) -> Self {
        Self {
            name,
            key_space,
            _marker: PhantomData,
        }
    }

    pub fn write(&mut self, value: T) -> io::Result<()> {
        let v = borsh::to_vec(&value)?;
        host::casper_write(Keyspace::Context(self.name.as_bytes()), 0, &v)
            .map_err(|_error| io::Error::new(io::ErrorKind::Other, "todo"))?;
        Ok(())
    }
}
impl<T: BorshDeserialize> Field<T> {
    pub fn read(&self) -> io::Result<Option<T>> {
        let mut read = None;
        host::casper_read(Keyspace::Context(self.name.as_bytes()), |size| {
            *(&mut read) = Some(Vec::new());
            reserve_vec_space(read.as_mut().unwrap(), size)
        })
        .map_err(|_error| io::Error::new(io::ErrorKind::Other, "todo"))?;
        match read {
            Some(read) => {
                let value = T::deserialize(&mut read.as_slice())?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }
}
