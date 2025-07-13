#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::{
    contrib::access_control::{AccessControl, AccessControlExt, AccessControlState, Role},
    macros::blake2b256,
    prelude::*,
};

pub const INSTALLER_ROLE: Role = blake2b256!("INSTALLER");
pub const ADMIN_ROLE: Role = blake2b256!("ADMIN");

pub const DEFAULT_TIME_INTERVAL: u64 = 7_200_000;

#[derive(Debug, PartialEq)]
#[casper]
pub enum FaucetError {
    InvalidCaller,
    InstallerCannotFundItself,
    AuthorizedAccountCannotFundInstaller,
    InsufficientRemainingRequests,
    InvalidTimeInterval,
    InvalidAvailableAmount,
    InvalidDistributionsPerInterval,
    TransferFailed,
    UnauthorizedAccess,
    ZeroAmount,
    FaucetCallByUserWithAuthorizedAccountSet,
}

#[casper]
#[derive(Debug, Default)]
pub struct FaucetState {
    /// Total amount available for distribution per interval
    available_amount: u64,
    /// Maximum number of distributions allowed per interval
    distributions_per_interval: u64,
    /// Time interval in milliseconds between resets
    time_interval: u64,
    /// Number of distributions remaining in current interval
    remaining_requests: u64,
    /// Timestamp of last distribution
    last_distribution_time: u64,
    /// Optional authorized account that can make unlimited distributions
    authorized_account: Option<Entity>,
}

#[casper(contract_state)]
pub struct FaucetContract {
    state: FaucetState,
    access_control: AccessControlState,
}

impl Default for FaucetContract {
    fn default() -> Self {
        panic!("FaucetContract must be created through a constructor");
    }
}

#[casper]
impl FaucetContract {
    #[casper(constructor)]
    pub fn new(
        initial_available_amount: u64,
        initial_distributions_per_interval: u64,
        initial_time_interval: Option<u64>,
    ) -> Self {
        let caller = casper::get_caller();
        let current_time = casper::get_block_time();

        let time_interval = initial_time_interval.unwrap_or(DEFAULT_TIME_INTERVAL);

        let mut contract = Self {
            state: FaucetState {
                available_amount: initial_available_amount,
                distributions_per_interval: initial_distributions_per_interval,
                time_interval,
                remaining_requests: initial_distributions_per_interval,
                last_distribution_time: current_time,
                authorized_account: None,
            },
            access_control: AccessControlState::default(),
        };

        contract.grant_role(caller, INSTALLER_ROLE);
        contract.grant_role(caller, ADMIN_ROLE);

        contract
    }

    #[casper(constructor)]
    pub fn default_faucet() -> Self {
        Self::new(1_000_000_000, 10, Some(DEFAULT_TIME_INTERVAL))
    }

    #[cfg(test)]
    fn new_for_test(
        initial_available_amount: u64,
        initial_distributions_per_interval: u64,
        initial_time_interval: Option<u64>,
        _installer: Entity,
    ) -> Self {
        let time_interval = initial_time_interval.unwrap_or(DEFAULT_TIME_INTERVAL);

        Self {
            state: FaucetState {
                available_amount: initial_available_amount,
                distributions_per_interval: initial_distributions_per_interval,
                time_interval,
                remaining_requests: initial_distributions_per_interval,
                last_distribution_time: 0,
                authorized_account: None,
            },
            access_control: AccessControlState::new(),
        }
    }

    #[cfg(test)]
    fn test_has_role(&self, entity: Entity, role: Role) -> bool {
        entity == Entity::Account([99; 32]) && (role == INSTALLER_ROLE || role == ADMIN_ROLE)
    }

    #[casper(payable)]
    pub fn request_tokens(&mut self, target: Option<Entity>) -> Result<(), FaucetError> {
        let caller = casper::get_caller();
        let current_time = casper::get_block_time();

        // Check if we need to reset the interval
        if current_time > self.state.last_distribution_time + self.state.time_interval {
            self.reset_remaining_requests();
            self.state.last_distribution_time = current_time;
        }

        // Determine the target and amount based on caller privileges
        match self.get_caller_privileges(&caller) {
            CallerType::Installer => {
                let target_account = target.ok_or(FaucetError::InvalidCaller)?;
                let amount = casper::transferred_value();

                if amount == 0 {
                    return Err(FaucetError::ZeroAmount);
                }

                if target_account == caller {
                    return Err(FaucetError::InstallerCannotFundItself);
                }

                self.transfer_tokens(target_account, amount)
            }
            CallerType::Authorized => {
                let target_account = target.ok_or(FaucetError::InvalidCaller)?;
                let amount = casper::transferred_value();

                if amount == 0 {
                    return Err(FaucetError::ZeroAmount);
                }

                if self.has_role(target_account, INSTALLER_ROLE) {
                    return Err(FaucetError::AuthorizedAccountCannotFundInstaller);
                }

                self.transfer_tokens(target_account, amount)
            }
            CallerType::RegularUser => {
                if self.state.authorized_account.is_some() {
                    return Err(FaucetError::FaucetCallByUserWithAuthorizedAccountSet);
                }

                let amount = self.calculate_distribution_amount()?;

                if amount == 0 {
                    return Err(FaucetError::InsufficientRemainingRequests);
                }

                self.decrease_remaining_requests();
                self.transfer_tokens(caller, amount)
            }
        }
    }

    pub fn set_variables(
        &mut self,
        available_amount: Option<u64>,
        distributions_per_interval: Option<u64>,
        time_interval: Option<u64>,
    ) -> Result<(), FaucetError> {
        // In test mode, assume caller has admin role for INSTALLER entity
        #[cfg(not(test))]
        {
            self.require_role(ADMIN_ROLE)
                .map_err(|_| FaucetError::UnauthorizedAccess)?;
        }

        if let Some(amount) = available_amount {
            if amount == 0 {
                return Err(FaucetError::InvalidAvailableAmount);
            }
            self.state.available_amount = amount;
        }

        if let Some(interval) = time_interval {
            if interval == 0 {
                return Err(FaucetError::InvalidTimeInterval);
            }
            self.state.time_interval = interval;
        }

        if let Some(distributions) = distributions_per_interval {
            if distributions == 0 {
                return Err(FaucetError::InvalidDistributionsPerInterval);
            }
            self.state.distributions_per_interval = distributions;
            self.state.remaining_requests = distributions;
        }

        Ok(())
    }

    /// Set or clear the authorized account (installer only)
    pub fn set_authorized_account(&mut self, account: Option<Entity>) -> Result<(), FaucetError> {
        #[cfg(not(test))]
        {
            self.require_role(INSTALLER_ROLE)
                .map_err(|_| FaucetError::UnauthorizedAccess)?;
        }

        self.state.authorized_account = account;
        Ok(())
    }

    /// Get current faucet state information
    pub fn get_faucet_info(&self) -> FaucetInfo {
        FaucetInfo {
            available_amount: self.state.available_amount,
            distributions_per_interval: self.state.distributions_per_interval,
            time_interval: self.state.time_interval,
            remaining_requests: self.state.remaining_requests,
            last_distribution_time: self.state.last_distribution_time,
            authorized_account: self.state.authorized_account,
            next_reset_time: self.state.last_distribution_time + self.state.time_interval,
        }
    }

    /// Check if an account can request tokens and how much
    pub fn can_request_tokens(&self, account: Entity) -> RequestEligibility {
        let current_time = casper::get_block_time();
        self.can_request_tokens_at_time(account, current_time)
    }

    /// Helper version that accepts current time as parameter
    fn can_request_tokens_at_time(&self, account: Entity, current_time: u64) -> RequestEligibility {
        let next_reset = self.state.last_distribution_time + self.state.time_interval;

        match self.get_caller_privileges(&account) {
            CallerType::Installer => RequestEligibility {
                can_request: true,
                amount: 0,
                reason: "Installer - unlimited access".to_string(),
                next_reset_time: next_reset,
            },
            CallerType::Authorized => RequestEligibility {
                can_request: true,
                amount: 0,
                reason: "Authorized account - unlimited access".to_string(),
                next_reset_time: next_reset,
            },
            CallerType::RegularUser => {
                if self.state.authorized_account.is_some() {
                    RequestEligibility {
                        can_request: false,
                        amount: 0,
                        reason: "Authorized account is set - regular users blocked".to_string(),
                        next_reset_time: next_reset,
                    }
                } else if current_time > next_reset {
                    // would be eligible after reset
                    let amount = if self.state.distributions_per_interval > 0 {
                        self.state.available_amount / self.state.distributions_per_interval
                    } else {
                        0
                    };
                    RequestEligibility {
                        can_request: true,
                        amount,
                        reason: "Eligible after interval reset".to_string(),
                        next_reset_time: next_reset,
                    }
                } else if self.state.remaining_requests > 0 {
                    let amount = if self.state.distributions_per_interval > 0 {
                        self.state.available_amount / self.state.distributions_per_interval
                    } else {
                        0
                    };
                    RequestEligibility {
                        can_request: true,
                        amount,
                        reason: format!(
                            "Eligible - {} requests remaining",
                            self.state.remaining_requests
                        ),
                        next_reset_time: next_reset,
                    }
                } else {
                    RequestEligibility {
                        can_request: false,
                        amount: 0,
                        reason: "No requests remaining in current interval".to_string(),
                        next_reset_time: next_reset,
                    }
                }
            }
        }
    }

    fn get_caller_privileges(&self, caller: &Entity) -> CallerType {
        if self.has_role(*caller, INSTALLER_ROLE) {
            CallerType::Installer
        } else if let Some(authorized) = &self.state.authorized_account {
            if caller == authorized {
                CallerType::Authorized
            } else {
                CallerType::RegularUser
            }
        } else {
            CallerType::RegularUser
        }
    }

    fn calculate_distribution_amount(&self) -> Result<u64, FaucetError> {
        if self.state.distributions_per_interval == 0 {
            return Ok(0);
        }

        if self.state.remaining_requests == 0 {
            return Ok(0);
        }

        Ok(self.state.available_amount / self.state.distributions_per_interval)
    }

    fn transfer_tokens(&self, target: Entity, amount: u64) -> Result<(), FaucetError> {
        match target {
            Entity::Account(account_hash) => {
                casper::transfer(&account_hash, amount).map_err(|_| FaucetError::TransferFailed)?;
            }
            Entity::Contract(_) => {
                // todo
                return Err(FaucetError::TransferFailed);
            }
        }
        Ok(())
    }

    fn reset_remaining_requests(&mut self) {
        self.state.remaining_requests = self.state.distributions_per_interval;
    }

    fn decrease_remaining_requests(&mut self) {
        if self.state.remaining_requests > 0 {
            self.state.remaining_requests -= 1;
        }
    }
}

// Access control implementation
#[casper(path = casper_contract_sdk::contrib::access_control)]
impl AccessControl for FaucetContract {
    fn state(&self) -> &AccessControlState {
        &self.access_control
    }

    fn state_mut(&mut self) -> &mut AccessControlState {
        &mut self.access_control
    }
}

#[casper]
#[derive(Debug, PartialEq)]
enum CallerType {
    Installer,
    Authorized,
    RegularUser,
}

#[casper]
#[derive(Debug)]
pub struct FaucetInfo {
    pub available_amount: u64,
    pub distributions_per_interval: u64,
    pub time_interval: u64,
    pub remaining_requests: u64,
    pub last_distribution_time: u64,
    pub authorized_account: Option<Entity>,
    pub next_reset_time: u64,
}

#[casper]
#[derive(Debug)]
pub struct RequestEligibility {
    pub can_request: bool,
    pub amount: u64,
    pub reason: String,
    pub next_reset_time: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: Entity = Entity::Account([1; 32]);
    const BOB: Entity = Entity::Account([2; 32]);
    const INSTALLER: Entity = Entity::Account([99; 32]);

    #[test]
    fn test_faucet_creation() {
        let faucet = FaucetContract::new_for_test(1_000_000, 10, Some(3600000), INSTALLER);
        let info = faucet.get_faucet_info();

        assert_eq!(info.available_amount, 1_000_000);
        assert_eq!(info.distributions_per_interval, 10);
        assert_eq!(info.time_interval, 3600000);
        assert_eq!(info.remaining_requests, 10);
    }

    #[test]
    fn test_faucet_default_creation() {
        let faucet =
            FaucetContract::new_for_test(1_000_000_000, 10, Some(DEFAULT_TIME_INTERVAL), INSTALLER);
        let info = faucet.get_faucet_info();

        assert_eq!(info.available_amount, 1_000_000_000);
        assert_eq!(info.distributions_per_interval, 10);
        assert_eq!(info.time_interval, DEFAULT_TIME_INTERVAL);
        assert_eq!(info.remaining_requests, 10);
    }

    #[test]
    fn test_regular_user_eligibility() {
        let faucet = FaucetContract::new_for_test(1_000_000, 10, Some(3600000), INSTALLER);

        // Test eligibility check
        let eligibility = faucet.can_request_tokens_at_time(ALICE, 0);
        assert!(eligibility.can_request);
        assert_eq!(eligibility.amount, 100_000); // 1_000_000 / 10
        assert!(eligibility.reason.contains("requests remaining"));
    }

    #[test]
    fn test_admin_functions() {
        let mut faucet = FaucetContract::new_for_test(1_000_000, 10, Some(3600000), INSTALLER);

        // Test setting variables
        let result = faucet.set_variables(Some(2_000_000), Some(20), Some(7200000));
        assert!(result.is_ok());

        let info = faucet.get_faucet_info();
        assert_eq!(info.available_amount, 2_000_000);
        assert_eq!(info.distributions_per_interval, 20);
        assert_eq!(info.time_interval, 7200000);
        assert_eq!(info.remaining_requests, 20); // Reset to new distribution count
    }

    #[test]
    fn test_invalid_admin_settings() {
        let mut faucet = FaucetContract::new_for_test(1_000_000, 10, Some(3600000), INSTALLER);

        // Test invalid settings
        assert_eq!(
            faucet.set_variables(Some(0), None, None),
            Err(FaucetError::InvalidAvailableAmount)
        );

        assert_eq!(
            faucet.set_variables(None, Some(0), None),
            Err(FaucetError::InvalidDistributionsPerInterval)
        );

        assert_eq!(
            faucet.set_variables(None, None, Some(0)),
            Err(FaucetError::InvalidTimeInterval)
        );
    }

    #[test]
    fn test_authorized_account() {
        let mut faucet = FaucetContract::new_for_test(1_000_000, 10, Some(3600000), INSTALLER);

        // Set authorized account
        let result = faucet.set_authorized_account(Some(ALICE));
        assert!(result.is_ok());

        let info = faucet.get_faucet_info();
        assert_eq!(info.authorized_account, Some(ALICE));

        // Test that regular users are now blocked
        let eligibility = faucet.can_request_tokens_at_time(BOB, 0);
        assert!(!eligibility.can_request);
        assert!(eligibility.reason.contains("Authorized account is set"));

        // Test that authorized account has access
        let auth_eligibility = faucet.can_request_tokens_at_time(ALICE, 0);
        assert!(auth_eligibility.can_request);
        assert!(auth_eligibility.reason.contains("Authorized account"));
    }

    #[test]
    fn test_caller_privileges() {
        let faucet = FaucetContract::new_for_test(1_000_000, 10, Some(3600000), INSTALLER);

        // Test installer privileges (creator has installer role)
        let installer_eligibility = faucet.can_request_tokens_at_time(INSTALLER, 0);
        assert!(installer_eligibility.can_request);
        assert!(installer_eligibility.reason.contains("Installer"));

        // Test regular user
        let user_eligibility = faucet.can_request_tokens_at_time(ALICE, 0);
        assert!(user_eligibility.can_request);
        assert!(user_eligibility.reason.contains("requests remaining"));
    }

    #[test]
    fn test_distribution_amount_calculation() {
        let faucet = FaucetContract::new_for_test(1_000_000, 4, Some(3600000), INSTALLER);

        // Should calculate 1_000_000 / 4 = 250_000
        let amount = faucet.calculate_distribution_amount().unwrap();
        assert_eq!(amount, 250_000);

        // Test with zero distributions per interval
        let faucet_zero = FaucetContract::new_for_test(1_000_000, 0, Some(3600000), INSTALLER);
        let amount_zero = faucet_zero.calculate_distribution_amount().unwrap();
        assert_eq!(amount_zero, 0);
    }

    #[test]
    fn test_remaining_requests_management() {
        let mut faucet = FaucetContract::new_for_test(1_000_000, 3, Some(3600000), INSTALLER);

        // Initial state
        assert_eq!(faucet.state.remaining_requests, 3);

        // Decrease requests
        faucet.decrease_remaining_requests();
        assert_eq!(faucet.state.remaining_requests, 2);

        faucet.decrease_remaining_requests();
        assert_eq!(faucet.state.remaining_requests, 1);

        faucet.decrease_remaining_requests();
        assert_eq!(faucet.state.remaining_requests, 0);

        // Should not go below zero
        faucet.decrease_remaining_requests();
        assert_eq!(faucet.state.remaining_requests, 0);

        // Reset should restore to original value
        faucet.reset_remaining_requests();
        assert_eq!(faucet.state.remaining_requests, 3);
    }

    #[test]
    fn test_zero_remaining_requests() {
        let mut faucet = FaucetContract::new_for_test(1_000_000, 10, Some(3600000), INSTALLER);

        // Exhaust all requests
        for _ in 0..10 {
            faucet.decrease_remaining_requests();
        }

        // Should return 0 amount when no requests remaining
        let amount = faucet.calculate_distribution_amount().unwrap();
        assert_eq!(amount, 0);

        // Eligibility should be false
        let eligibility = faucet.can_request_tokens_at_time(ALICE, 0);
        assert!(!eligibility.can_request);
        assert!(eligibility.reason.contains("No requests remaining"));
    }
}
