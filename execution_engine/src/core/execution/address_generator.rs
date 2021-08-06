//! Address generator is used to generate unique addresses that have length of 32 bytes.

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaChaRng;

use casper_types::{AccessRights, Phase, URef};

use crate::core::{Address, ADDRESS_LENGTH};

const SEED_LENGTH: usize = 32;

/// An `AddressGenerator` generates `URef` addresses.
pub struct AddressGenerator(ChaChaRng);

impl AddressGenerator {
    /// Creates an [`AddressGenerator`] from a 32-byte hash digest and [`Phase`].
    pub fn new(hash: &[u8], phase: Phase) -> AddressGenerator {
        AddressGeneratorBuilder::new()
            .seed_with(hash)
            .seed_with(&[phase as u8])
            .build()
    }

    /// Creates a new [`Address`] by using an internal instance of PRNG.
    pub fn create_address(&mut self) -> Address {
        let mut buff = [0u8; ADDRESS_LENGTH];
        self.0.fill_bytes(&mut buff);
        buff
    }

    /// Creates a new [`Address`] by hashing an output from [`AddressGenerator::create_address`]
    /// with a blake2b256.
    pub fn new_hash_address(&mut self) -> Address {
        // TODO: this appears to duplicate the logic of AddressGeneratorBuilder::build()
        let pre_hash_bytes = self.create_address();
        // NOTE: Unwrap below is assumed safe as output size of `ADDRESS_LENGTH` is a valid value.
        let mut hasher = VarBlake2b::new(ADDRESS_LENGTH).unwrap();
        hasher.update(&pre_hash_bytes);
        let mut hash_bytes = [0; ADDRESS_LENGTH];
        hasher.finalize_variable(|hash| hash_bytes.clone_from_slice(hash));
        hash_bytes
    }

    /// Creates a new [`URef`] with a new address generated.
    pub fn new_uref(&mut self, access_rights: AccessRights) -> URef {
        let addr = self.create_address();
        URef::new(addr, access_rights)
    }
}

/// A builder for [`AddressGenerator`].
#[derive(Default)]
pub struct AddressGeneratorBuilder {
    data: Vec<u8>,
}

impl AddressGeneratorBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Default::default()
    }

    /// Extends the seed with more data.
    pub fn seed_with(mut self, bytes: &[u8]) -> Self {
        self.data.extend(bytes);
        self
    }

    /// Creates a new [`AddressGenerator`].
    ///
    /// This method hashes the seed bytes, and seeds the PRNG with it.
    pub fn build(self) -> AddressGenerator {
        let mut seed: [u8; SEED_LENGTH] = [0u8; SEED_LENGTH];
        // NOTE: Unwrap below is assumed safe as output size of `SEED_LENGTH` is a valid value.
        let mut hasher = VarBlake2b::new(SEED_LENGTH).unwrap();
        hasher.update(self.data);
        hasher.finalize_variable(|hash| seed.clone_from_slice(hash));
        AddressGenerator(ChaChaRng::from_seed(seed))
    }
}

#[cfg(test)]
mod tests {
    use casper_types::Phase;

    use super::AddressGenerator;

    const DEPLOY_HASH_1: [u8; 32] = [1u8; 32];
    const DEPLOY_HASH_2: [u8; 32] = [2u8; 32];

    #[test]
    fn should_generate_different_numbers_for_different_seeds() {
        let mut ag_a = AddressGenerator::new(&DEPLOY_HASH_1, Phase::Session);
        let mut ag_b = AddressGenerator::new(&DEPLOY_HASH_2, Phase::Session);
        let random_a = ag_a.create_address();
        let random_b = ag_b.create_address();

        assert_ne!(random_a, random_b)
    }

    #[test]
    fn should_generate_same_numbers_for_same_seed() {
        let mut ag_a = AddressGenerator::new(&DEPLOY_HASH_1, Phase::Session);
        let mut ag_b = AddressGenerator::new(&DEPLOY_HASH_1, Phase::Session);
        let random_a = ag_a.create_address();
        let random_b = ag_b.create_address();

        assert_eq!(random_a, random_b)
    }

    #[test]
    fn should_not_generate_same_numbers_for_different_phase() {
        let mut ag_a = AddressGenerator::new(&DEPLOY_HASH_1, Phase::Payment);
        let mut ag_b = AddressGenerator::new(&DEPLOY_HASH_1, Phase::Session);
        let mut ag_c = AddressGenerator::new(&DEPLOY_HASH_1, Phase::FinalizePayment);
        let random_a = ag_a.create_address();
        let random_b = ag_b.create_address();
        let random_c = ag_c.create_address();

        assert_ne!(
            random_a, random_b,
            "different phase should have different output"
        );

        assert_ne!(
            random_a, random_c,
            "different phase should have different output"
        );
    }
}
