pub(crate) fn compute_blake2b256(bytes: &[u8]) -> [u8; 32] {
    let mut context = blake2_rfc::blake2b::Blake2b::new(32);
    context.update(bytes);
    context.finalize().as_bytes().try_into().unwrap()
}
