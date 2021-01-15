use std::{cell::RefCell, rc::Rc};

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaChaRng;

use casper_types::{AccessRights, DeployHash, Phase, URef};

use crate::core::{Address, ADDRESS_LENGTH};

const SEED_LENGTH: usize = 32;

/// An `AddressGenerator` generates `URef` addresses.
pub struct AddressGenerator(ChaChaRng);

impl AddressGenerator {
    /// Creates an [`AddressGenerator`] from a 32-byte hash digest and [`Phase`].
    pub fn new(hash: &[u8], phase: Phase) -> AddressGenerator {
        AddressGeneratorBuilder::new()
            .seed_with(&hash)
            .seed_with(&[phase as u8])
            .build()
    }

    pub fn create_address(&mut self) -> Address {
        let mut buff = [0u8; ADDRESS_LENGTH];
        self.0.fill_bytes(&mut buff);
        buff
    }
}

/// A builder for [`AddressGenerator`].
#[derive(Default)]
pub struct AddressGeneratorBuilder {
    data: Vec<u8>,
}

impl AddressGeneratorBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn seed_with(mut self, bytes: &[u8]) -> Self {
        self.data.extend(bytes);
        self
    }

    pub fn build(self) -> AddressGenerator {
        let mut seed: [u8; SEED_LENGTH] = [0u8; SEED_LENGTH];
        // NOTE: Unwrap below is assumed safe as output size of `SEED_LENGTH` is a valid value.
        let mut hasher = VarBlake2b::new(SEED_LENGTH).unwrap();
        hasher.update(self.data);
        hasher.finalize_variable(|hash| seed.clone_from_slice(hash));
        AddressGenerator(ChaChaRng::from_seed(seed))
    }
}

pub struct AddressGenerators {
    phase: Phase,
    deploy_hash: DeployHash,
    hash: Rc<RefCell<AddressGenerator>>,
    uref: Rc<RefCell<AddressGenerator>>,
    transfer: Rc<RefCell<AddressGenerator>>,
}

impl AddressGenerators {
    pub fn new(deploy_hash: &DeployHash, phase: Phase) -> Self {
        let bytes = deploy_hash.value();
        AddressGenerators {
            phase,
            deploy_hash: *deploy_hash,
            hash: Rc::new(RefCell::new(AddressGenerator::new(&bytes, phase))),
            uref: Rc::new(RefCell::new(AddressGenerator::new(&bytes, phase))),
            transfer: Rc::new(RefCell::new(AddressGenerator::new(&bytes, phase))),
        }
    }

    pub fn deploy_hash(&self) -> &DeployHash {
        &self.deploy_hash
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn hash(&mut self) -> Rc<RefCell<AddressGenerator>> {
        Rc::clone(&self.hash)
    }

    pub fn uref(&mut self) -> Rc<RefCell<AddressGenerator>> {
        Rc::clone(&self.uref)
    }

    pub fn transfer(&mut self) -> Rc<RefCell<AddressGenerator>> {
        Rc::clone(&self.transfer)
    }

    /// Generates a new hash address.
    pub fn new_hash_address(&mut self) -> Address {
        // TODO: this method basically duplicates the AddressGeneratorBuilder::build method; one or
        // the other is likely redundant.

        let pre_hash_bytes = self.hash.borrow_mut().create_address();
        // NOTE: Unwrap below is assumed safe as output size of `ADDRESS_LENGTH` is a valid value.
        let mut hasher = VarBlake2b::new(ADDRESS_LENGTH).unwrap();
        hasher.update(&pre_hash_bytes);
        let mut hash_bytes = [0; ADDRESS_LENGTH];
        hasher.finalize_variable(|hash| hash_bytes.clone_from_slice(hash));
        hash_bytes
    }

    /// Creates a new uref with specified AccessRights.
    pub fn new_uref(&mut self, access_rights: AccessRights) -> URef {
        let addr = self.uref.borrow_mut().create_address();
        URef::new(addr, access_rights)
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
