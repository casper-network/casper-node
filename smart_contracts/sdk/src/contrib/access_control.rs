#[allow(unused_imports)]
use crate as casper_contract_sdk; // Workaround for absolute crate path in derive CasperABI macro

use casper_contract_macros::casper;

use crate::{
    casper::{self, Entity},
    collections::{sorted_vector::SortedVector, Map},
};

/// A role is a unique identifier for a specific permission or set of permissions.
///
/// You can use `blake2b256` macro to generate a unique identifier for a role at compile time.
pub type Role = [u8; 32];

/// A role is a unique identifier for a specific permission or set of permissions.
const ROLES_PREFIX: &str = "roles";

/// The state of the access control contract, which contains a mapping of entities to their roles.
#[casper(path = "crate")]
pub struct AccessControlState {
    roles: Map<Entity, SortedVector<Role>>,
}

impl AccessControlState {
    /// Creates a new instance of `AccessControlState`.
    pub fn new() -> Self {
        Self {
            roles: Map::new(ROLES_PREFIX),
        }
    }
}

impl Default for AccessControlState {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents the possible errors that can occur during access control operations.
#[casper(path = "crate")]
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum AccessControlError {
    /// The caller is not authorized to perform the action.
    NotAuthorized,
}

/// The AccessControl trait provides a simple role-based access control mechanism.
/// It allows for multiple roles to be assigned to an account, and provides functions to check,
/// grant, and revoke roles.
/// It also provides functions to check if the caller has a specific role or any of a set of roles.
///
/// The roles are stored in a `Map` where the key is the account address and the value is a
/// `SortedVector` of roles.
///
/// None of these methods are turned into smart contract entry points, so they are not exposed
/// accidentally.
///
/// The `AccessControl` trait is designed to be used with the `casper` macro, which generates
/// the necessary boilerplate code for the contract.
#[casper(path = "crate", export = true)]
pub trait AccessControl {
    /// The state of the contract, which contains the roles.
    #[casper(private)]
    fn state(&self) -> &AccessControlState;
    /// The mutable state of the contract, which allows modifying the roles.
    #[casper(private)]
    fn state_mut(&mut self) -> &mut AccessControlState;

    /// Checks if the given account has the specified role.
    #[casper(private)]
    fn has_role(&self, entity: Entity, role: Role) -> bool {
        match self.state().roles.get(&entity) {
            Some(roles) => roles.contains(&role),
            None => false,
        }
    }

    #[casper(private)]
    fn has_any_role(&self, entity: Entity, roles: &[Role]) -> bool {
        match self.state().roles.get(&entity) {
            Some(roles_vec) => roles_vec.iter().any(|r| roles.contains(&r)),
            None => false,
        }
    }

    /// Grants a role to an account. If the account already has the role, it does nothing.
    #[casper(private)]
    fn grant_role(&mut self, entity: Entity, role: Role) {
        match self.state_mut().roles.get(&entity) {
            Some(mut roles) => {
                if roles.contains(&role) {
                    return;
                }
                roles.push(role);
            }
            None => {
                let mut roles = SortedVector::new(format!(
                    "{ROLES_PREFIX}-{:02x}{}",
                    entity.tag(),
                    base16::encode_lower(&entity.address())
                ));
                roles.push(role);
                self.state_mut().roles.insert(&entity, &roles);
            }
        }
    }

    /// Revokes a role from an account. If the account does not have the role, it does nothing.
    #[casper(private)]
    fn revoke_role(&mut self, entity: Entity, role: Role) {
        if let Some(mut roles) = self.state_mut().roles.get(&entity) {
            roles.retain(|r| r != &role);
        }
    }

    /// Checks if the caller has the specified role and reverts if not.
    #[casper(private)]
    fn require_role(&self, role: Role) -> Result<(), AccessControlError> {
        let caller = casper::get_caller();
        if !self.has_role(caller, role) {
            // Caller does not have specified role.
            return Err(AccessControlError::NotAuthorized);
        }
        Ok(())
    }

    /// Checks if the caller has any of the specified roles and reverts if not.
    #[casper(private)]
    fn require_any_role(&self, roles: &[Role]) -> Result<(), AccessControlError> {
        let caller = casper::get_caller();
        if !self.has_any_role(caller, roles) {
            // Caller does not have any of the specified roles.
            return Err(AccessControlError::NotAuthorized);
        }
        Ok(())
    }
}
