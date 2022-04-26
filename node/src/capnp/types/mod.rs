mod account_hash;
mod action_thresholds;
mod associated_keys;
mod block;
mod block_body;
mod block_header;
mod common;
mod deploy_hash;
mod digest;
mod era;
mod public_key;
mod weight;

pub trait ToCapnpBuilder<T> {
    fn try_to_builder(&mut self, object: &T) -> Result<(), super::SerializeError>;
}

pub trait FromCapnpReader<T> {
    fn try_from_reader(&self) -> Result<T, super::DeserializeError>;
}

#[cfg(test)]
use std::convert::TryInto;

#[cfg(test)]
pub(crate) fn random_byte() -> u8 {
    random_bytes(1)[0]
}

#[cfg(test)]
pub(crate) fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0; len];
    getrandom::getrandom(&mut buf).expect("should get random");
    buf
}

#[cfg(test)]
pub(crate) fn random_u64() -> u64 {
    let random_u64_bytes = random_bytes(8);
    u64::from_le_bytes(random_u64_bytes.try_into().unwrap())
}
