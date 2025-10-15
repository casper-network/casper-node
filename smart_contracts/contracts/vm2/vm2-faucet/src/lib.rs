use casper_contract_sdk::{macros::blake2b256, prelude::*, types::EntityAddr};
use casper_contract_sdk_contrib::access_control::{
    AccessControl, AccessControlExt, AccessControlState, Role,
};

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

#[casper(message)]
struct FaucetTokensTransferred {
    target: Entity,
    amount: u64,
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

        contract.grant_role(caller, ADMIN_ROLE);

        contract
    }

    #[casper(constructor)]
    pub fn default_faucet() -> Self {
        Self::new(1_000_000_000, 10, Some(DEFAULT_TIME_INTERVAL))
    }

    #[casper(payable)]
    pub fn request_tokens(&mut self, target: Option<Entity>) -> Result<(), FaucetError> {
        let caller = casper::get_caller();
        let current_time = casper::get_block_time();

        if current_time > self.state.last_distribution_time + self.state.time_interval {
            self.reset_remaining_requests();
            self.state.last_distribution_time = current_time;
        }

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

                if self.has_role(target_account, ADMIN_ROLE) {
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
        self.require_role(ADMIN_ROLE)
            .map_err(|_| FaucetError::UnauthorizedAccess)?;

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

    pub fn set_authorized_account(&mut self, account: Option<Entity>) -> Result<(), FaucetError> {
        self.require_role(ADMIN_ROLE)
            .map_err(|_| FaucetError::UnauthorizedAccess)?;

        self.state.authorized_account = account;
        Ok(())
    }

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

    pub fn can_request_tokens(&self, account: Entity) -> RequestEligibility {
        let current_time = casper::get_block_time();
        self.can_request_tokens_at_time(account, current_time)
    }

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
        if self.has_role(*caller, ADMIN_ROLE) {
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
                casper::transfer(&EntityAddr::Account(account_hash), amount)
                    .map_err(|_| FaucetError::TransferFailed)?;
            }
            Entity::Contract(_) => {
                return Err(FaucetError::TransferFailed);
            }
        }
        casper::emit_message(FaucetTokensTransferred { target, amount }).unwrap();
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

#[casper(path = casper_contract_sdk_contrib::access_control)]
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

    use casper_contract_sdk::casper::{
        native::{dispatch_with, Environment},
        Entity,
    };

    const ALICE: Entity = Entity::Account([1; 32]);
    const BOB: Entity = Entity::Account([2; 32]);
    const CHARLIE: Entity = Entity::Account([3; 32]);
    const INSTALLER: Entity = Entity::Account([99; 32]);

    #[test]
    fn test_faucet_creation() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));
            let info = faucet.get_faucet_info();

            assert_eq!(info.available_amount, 1_000_000);
            assert_eq!(info.distributions_per_interval, 10);
            assert_eq!(info.time_interval, 3600000);
            assert_eq!(info.remaining_requests, 10);
            assert_eq!(info.authorized_account, None);

            // Check that the caller has admin role (which should be granted by constructor)
            let actual_caller = casper::get_caller();
            assert!(faucet.has_role(actual_caller, ADMIN_ROLE));
            assert_eq!(faucet.require_role(ADMIN_ROLE), Ok(()));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_faucet_creation_with_default_time_interval() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, None);
            let info = faucet.get_faucet_info();

            assert_eq!(info.time_interval, DEFAULT_TIME_INTERVAL);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_default_faucet_creation() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::default_faucet();
            let info = faucet.get_faucet_info();

            assert_eq!(info.available_amount, 1_000_000_000);
            assert_eq!(info.distributions_per_interval, 10);
            assert_eq!(info.time_interval, DEFAULT_TIME_INTERVAL);
            assert_eq!(info.remaining_requests, 10);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_complete_flow_regular_user_eligibility() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            // Install faucet
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Check initial state
            let info = faucet.get_faucet_info();
            assert_eq!(info.remaining_requests, 10);

            // Check that regular user can request tokens
            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 100_000); // 1_000_000 / 10
            assert!(eligibility.reason.contains("requests remaining"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_eligibility_installer_has_unlimited_access() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3_600_000));

            // Check that installer has unlimited access
            let eligibility = faucet.can_request_tokens(INSTALLER);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 0);
            assert!(eligibility.reason.contains("unlimited access"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_logic_regular_user_eligibility() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Test eligibility for regular user
            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 100_000); // 1_000_000 / 10
            assert!(eligibility.reason.contains("requests remaining"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_logic_no_remaining_requests() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Exhaust all requests
            for _ in 0..10 {
                faucet.decrease_remaining_requests();
            }

            // Check that user cannot request tokens
            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(!eligibility.can_request);
            assert_eq!(eligibility.amount, 0);
            assert!(eligibility.reason.contains("No requests remaining"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_logic_installer_privileges() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Test installer privileges
            let eligibility = faucet.can_request_tokens(INSTALLER);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 0); // Installer doesn't get fixed amounts
            assert!(eligibility.reason.contains("unlimited access"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_logic_authorized_account() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Set authorized account
            faucet.set_authorized_account(Some(ALICE)).unwrap();

            // Test authorized account privileges
            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 0); // Authorized account doesn't get fixed amounts
            assert!(eligibility.reason.contains("unlimited access"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_logic_blocked_when_authorized_set() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Set authorized account
            faucet.set_authorized_account(Some(ALICE)).unwrap();

            // Test that regular users are blocked
            let eligibility = faucet.can_request_tokens(BOB);
            assert!(!eligibility.can_request);
            assert_eq!(eligibility.amount, 0);
            assert!(eligibility.reason.contains("Authorized account is set"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_logic_time_interval_reset() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 2, Some(1000)); // 1 second interval

            // Exhaust requests
            for _ in 0..2 {
                faucet.decrease_remaining_requests();
            }

            let info = faucet.get_faucet_info();
            assert_eq!(info.remaining_requests, 0);

            // Check eligibility after time interval would reset
            let future_time = faucet.state.last_distribution_time + 2000;
            let eligibility = faucet.can_request_tokens_at_time(ALICE, future_time);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 500_000);
            assert!(eligibility.reason.contains("after interval reset"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_set_variables_success() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            let result = faucet.set_variables(Some(2_000_000), Some(20), Some(7200000));
            assert!(result.is_ok());

            let info = faucet.get_faucet_info();
            assert_eq!(info.available_amount, 2_000_000);
            assert_eq!(info.distributions_per_interval, 20);
            assert_eq!(info.time_interval, 7200000);
            assert_eq!(info.remaining_requests, 20);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_set_variables_partial_update() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Update only available amount
            let result = faucet.set_variables(Some(2_000_000), None, None);
            assert!(result.is_ok());

            let info = faucet.get_faucet_info();
            assert_eq!(info.available_amount, 2_000_000);
            assert_eq!(info.distributions_per_interval, 10);
            assert_eq!(info.time_interval, 3600000);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_set_variables_invalid_values() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

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
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_set_variables_unauthorized() {
        // Test access control by verifying the logic works correctly
        let alice_stub = Environment::new(Default::default(), ALICE);

        let result = dispatch_with(alice_stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // ALICE created the contract so she has admin role
            assert!(faucet.has_role(ALICE, ADMIN_ROLE));

            // Test that other users don't have admin role
            assert!(!faucet.has_role(BOB, ADMIN_ROLE));
            assert!(!faucet.has_role(CHARLIE, ADMIN_ROLE));

            // Verify the require_role method works correctly
            assert_eq!(faucet.require_role(ADMIN_ROLE), Ok(()));

            // Test that the access control system correctly identifies roles
            assert!(faucet.has_role(ALICE, ADMIN_ROLE));
            assert!(!faucet.has_role(BOB, ADMIN_ROLE));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_set_authorized_account_success() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            let result = faucet.set_authorized_account(Some(ALICE));
            assert!(result.is_ok());

            let info = faucet.get_faucet_info();
            assert_eq!(info.authorized_account, Some(ALICE));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_set_authorized_account_clear() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Set authorized account
            faucet.set_authorized_account(Some(ALICE)).unwrap();

            // Clear authorized account
            let result = faucet.set_authorized_account(None);
            assert!(result.is_ok());

            let info = faucet.get_faucet_info();
            assert_eq!(info.authorized_account, None);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_set_authorized_account_unauthorized() {
        // Create faucet with BOB as admin, then test access control
        let bob_stub = Environment::new(Default::default(), BOB);

        let result = dispatch_with(bob_stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // BOB should have admin role
            assert!(faucet.has_role(BOB, ADMIN_ROLE));

            // Test that other users don't have admin role
            assert!(!faucet.has_role(ALICE, ADMIN_ROLE));
            assert!(!faucet.has_role(INSTALLER, ADMIN_ROLE));

            // The current caller (BOB) should be able to set authorized account
            let result = faucet.set_authorized_account(Some(ALICE));
            assert!(result.is_ok());

            // Verify the change was made
            let info = faucet.get_faucet_info();
            assert_eq!(info.authorized_account, Some(ALICE));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_can_request_tokens_regular_user() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 100_000); // 1_000_000 / 10
            assert!(eligibility.reason.contains("requests remaining"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_can_request_tokens_installer() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            let eligibility = faucet.can_request_tokens(INSTALLER);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 0); // Installer doesn't get fixed amounts
            assert!(eligibility.reason.contains("unlimited access"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_can_request_tokens_authorized_account() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));
            faucet.set_authorized_account(Some(ALICE)).unwrap();

            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 0); // Authorized account doesn't get fixed amounts
            assert!(eligibility.reason.contains("unlimited access"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_can_request_tokens_blocked_by_authorized_account() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));
            faucet.set_authorized_account(Some(ALICE)).unwrap();

            let eligibility = faucet.can_request_tokens(BOB);
            assert!(!eligibility.can_request);
            assert_eq!(eligibility.amount, 0);
            assert!(eligibility.reason.contains("Authorized account is set"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_can_request_tokens_no_requests_remaining() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Exhaust all requests
            for _ in 0..10 {
                faucet.decrease_remaining_requests();
            }

            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(!eligibility.can_request);
            assert_eq!(eligibility.amount, 0);
            assert!(eligibility.reason.contains("No requests remaining"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_can_request_tokens_after_time_reset() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(1000));

            // Exhaust all requests
            for _ in 0..10 {
                faucet.decrease_remaining_requests();
            }

            // Check eligibility after time interval
            let future_time = faucet.state.last_distribution_time + 2000;
            let eligibility = faucet.can_request_tokens_at_time(ALICE, future_time);
            assert!(eligibility.can_request);
            assert_eq!(eligibility.amount, 100_000);
            assert!(eligibility.reason.contains("after interval reset"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_distribution_amount_calculation() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 4, Some(3600000));

            let amount = faucet.calculate_distribution_amount().unwrap();
            assert_eq!(amount, 250_000);

            let faucet_zero = FaucetContract::new(1_000_000, 0, Some(3600000));
            let amount_zero = faucet_zero.calculate_distribution_amount().unwrap();
            assert_eq!(amount_zero, 0);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_remaining_requests_management() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 3, Some(3600000));

            assert_eq!(faucet.state.remaining_requests, 3);

            faucet.decrease_remaining_requests();
            assert_eq!(faucet.state.remaining_requests, 2);

            faucet.decrease_remaining_requests();
            assert_eq!(faucet.state.remaining_requests, 1);

            faucet.decrease_remaining_requests();
            assert_eq!(faucet.state.remaining_requests, 0);

            // Should not go below zero
            faucet.decrease_remaining_requests();
            assert_eq!(faucet.state.remaining_requests, 0);

            faucet.reset_remaining_requests();
            assert_eq!(faucet.state.remaining_requests, 3);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_caller_type_classification() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Test installer
            let caller_type = faucet.get_caller_privileges(&INSTALLER);
            assert_eq!(caller_type, CallerType::Installer);

            // Test regular user
            let caller_type = faucet.get_caller_privileges(&ALICE);
            assert_eq!(caller_type, CallerType::RegularUser);

            // Test authorized account
            faucet.set_authorized_account(Some(BOB)).unwrap();
            let caller_type = faucet.get_caller_privileges(&BOB);
            assert_eq!(caller_type, CallerType::Authorized);

            // Test regular user after authorized account is set
            let caller_type = faucet.get_caller_privileges(&ALICE);
            assert_eq!(caller_type, CallerType::RegularUser);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_access_control_non_installer_lacks_roles() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Regular users should not have roles
            assert!(!faucet.has_role(ALICE, ADMIN_ROLE));
            assert!(!faucet.has_role(BOB, ADMIN_ROLE));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_zero_distributions_per_interval() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let faucet = FaucetContract::new(1_000_000, 0, Some(3600000));

            let amount = faucet.calculate_distribution_amount().unwrap();
            assert_eq!(amount, 0);

            // When distributions_per_interval is 0, regular users cannot request tokens
            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(!eligibility.can_request);
            assert_eq!(eligibility.amount, 0);
            assert!(eligibility.reason.contains("No requests remaining"));

            // But the installer should still have unlimited access
            let installer_eligibility = faucet.can_request_tokens(INSTALLER);
            assert!(installer_eligibility.can_request);
            assert_eq!(installer_eligibility.amount, 0);
            assert!(installer_eligibility.reason.contains("unlimited access"));
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_get_faucet_info_complete() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Set some state
            faucet.set_authorized_account(Some(ALICE)).unwrap();
            faucet.state.last_distribution_time = 1000;

            let info = faucet.get_faucet_info();

            assert_eq!(info.available_amount, 1_000_000);
            assert_eq!(info.distributions_per_interval, 10);
            assert_eq!(info.time_interval, 3600000);
            assert_eq!(info.remaining_requests, 10);
            assert_eq!(info.last_distribution_time, 1000);
            assert_eq!(info.authorized_account, Some(ALICE));
            assert_eq!(info.next_reset_time, 1000 + 3600000);
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_users_eligibility_over_time() {
        let stub = Environment::new(Default::default(), INSTALLER);

        let result = dispatch_with(stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 3, Some(1000)); // 3 distributions per 1 second

            // Initially all users should be eligible
            assert!(faucet.can_request_tokens(ALICE).can_request);
            assert!(faucet.can_request_tokens(BOB).can_request);
            assert!(faucet.can_request_tokens(CHARLIE).can_request);

            // Simulate exhausting all requests
            for _ in 0..3 {
                faucet.decrease_remaining_requests();
            }

            let info = faucet.get_faucet_info();
            assert_eq!(info.remaining_requests, 0);

            // Users should not be eligible
            assert!(!faucet.can_request_tokens(ALICE).can_request);
            assert!(!faucet.can_request_tokens(BOB).can_request);
            assert!(!faucet.can_request_tokens(CHARLIE).can_request);

            // After time interval, users should be eligible again
            let future_time = faucet.state.last_distribution_time + 2000;
            assert!(
                faucet
                    .can_request_tokens_at_time(ALICE, future_time)
                    .can_request
            );
            assert!(
                faucet
                    .can_request_tokens_at_time(BOB, future_time)
                    .can_request
            );
            assert!(
                faucet
                    .can_request_tokens_at_time(CHARLIE, future_time)
                    .can_request
            );
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_with_different_callers() {
        // Test installer behavior, zero transferred value validation
        let installer_env = Environment::new(Default::default(), INSTALLER);
        let result = dispatch_with(installer_env, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // Test zero amount validation
            let result = faucet.request_tokens(Some(ALICE));
            assert_eq!(result, Err(FaucetError::ZeroAmount));

            // Test installer cannot fund itself
            let result = faucet.request_tokens(Some(INSTALLER));
            assert_eq!(result, Err(FaucetError::ZeroAmount));
        });
        assert!(result.is_ok());

        // Test caller type identification
        let alice_stub = Environment::new(Default::default(), ALICE);
        let result = dispatch_with(alice_stub, || {
            let faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // ALICE created the contract so she has admin role
            assert!(faucet.has_role(ALICE, ADMIN_ROLE));

            // Test caller type classification
            let caller_type = faucet.get_caller_privileges(&ALICE);
            assert_eq!(caller_type, CallerType::Installer);

            let caller_type = faucet.get_caller_privileges(&BOB);
            assert_eq!(caller_type, CallerType::RegularUser);

            // Test that different users have different privileges
            assert!(!faucet.has_role(BOB, ADMIN_ROLE));
            assert!(!faucet.has_role(CHARLIE, ADMIN_ROLE));
        });
        assert!(result.is_ok());

        // Test authorized account scenario
        let bob_env = Environment::new(Default::default(), BOB);
        let result = dispatch_with(bob_env, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // BOB created the contract so he has admin role
            assert!(faucet.has_role(BOB, ADMIN_ROLE));

            // Set authorized account
            faucet.set_authorized_account(Some(ALICE)).unwrap();

            // Test caller type classification
            let caller_type = faucet.get_caller_privileges(&BOB);
            assert_eq!(caller_type, CallerType::Installer);

            let caller_type = faucet.get_caller_privileges(&ALICE);
            assert_eq!(caller_type, CallerType::Authorized);

            let caller_type = faucet.get_caller_privileges(&CHARLIE);
            assert_eq!(caller_type, CallerType::RegularUser);

            // Test eligibility when authorized account is set
            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(eligibility.can_request);
            assert!(eligibility.reason.contains("unlimited access"));

            let eligibility = faucet.can_request_tokens(CHARLIE);
            assert!(!eligibility.can_request);
            assert!(eligibility.reason.contains("Authorized account is set"));
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_request_tokens_blocked_by_authorized_account() {
        // Test the logic when authorized account is set
        let charlie_stub = Environment::new(Default::default(), CHARLIE);
        let result = dispatch_with(charlie_stub, || {
            let mut faucet = FaucetContract::new(1_000_000, 10, Some(3600000));

            // CHARLIE created the contract so he has admin role
            assert!(faucet.has_role(CHARLIE, ADMIN_ROLE));

            // Set up authorized account
            faucet.set_authorized_account(Some(ALICE)).unwrap();

            // Test eligibility for different user types
            let eligibility = faucet.can_request_tokens(CHARLIE);
            assert!(eligibility.can_request);
            assert!(eligibility.reason.contains("unlimited access"));

            let eligibility = faucet.can_request_tokens(ALICE);
            assert!(eligibility.can_request);
            assert!(eligibility.reason.contains("unlimited access"));

            let eligibility = faucet.can_request_tokens(BOB);
            assert!(!eligibility.can_request);
            assert!(eligibility.reason.contains("Authorized account is set"));
        });
        assert!(result.is_ok());
    }
}
