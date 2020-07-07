use std::{fmt, iter};

use num_traits::Zero;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

use crate::components::contract_runtime::shared::wasm_costs::WasmCosts;
use crate::components::contract_runtime::shared::{
    motes::Motes, newtypes::Blake2bHash, TypeMismatch,
};
use crate::components::contract_runtime::storage::global_state::CommitResult;
use types::{account::AccountHash, bytesrepr, Key, ProtocolVersion, U512};

use crate::components::contract_runtime::core::engine_state::execution_effect::ExecutionEffect;

pub const PLACEHOLDER_KEY: Key = Key::Hash([0u8; 32]);
pub const POS_BONDING_PURSE: &str = "pos_bonding_purse";
pub const POS_PAYMENT_PURSE: &str = "pos_payment_purse";
pub const POS_REWARDS_PURSE: &str = "pos_rewards_purse";

pub enum GenesisResult {
    RootNotFound,
    KeyNotFound(Key),
    TypeMismatch(TypeMismatch),
    Serialization(bytesrepr::Error),
    Success {
        post_state_hash: Blake2bHash,
        effect: ExecutionEffect,
    },
}

impl fmt::Display for GenesisResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            GenesisResult::RootNotFound => write!(f, "Root not found"),
            GenesisResult::KeyNotFound(key) => write!(f, "Key not found: {}", key),
            GenesisResult::TypeMismatch(type_mismatch) => {
                write!(f, "Type mismatch: {:?}", type_mismatch)
            }
            GenesisResult::Serialization(error) => write!(f, "Serialization error: {:?}", error),
            GenesisResult::Success {
                post_state_hash,
                effect,
            } => write!(f, "Success: {} {:?}", post_state_hash, effect),
        }
    }
}

impl GenesisResult {
    pub fn from_commit_result(commit_result: CommitResult, effect: ExecutionEffect) -> Self {
        match commit_result {
            CommitResult::RootNotFound => GenesisResult::RootNotFound,
            CommitResult::KeyNotFound(key) => GenesisResult::KeyNotFound(key),
            CommitResult::TypeMismatch(type_mismatch) => GenesisResult::TypeMismatch(type_mismatch),
            CommitResult::Serialization(error) => GenesisResult::Serialization(error),
            CommitResult::Success { state_root, .. } => GenesisResult::Success {
                post_state_hash: state_root,
                effect,
            },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GenesisAccount {
    account_hash: AccountHash,
    balance: Motes,
    bonded_amount: Motes,
}

impl GenesisAccount {
    pub fn new(account_hash: AccountHash, balance: Motes, bonded_amount: Motes) -> Self {
        GenesisAccount {
            account_hash,
            balance,
            bonded_amount,
        }
    }

    pub fn account_hash(&self) -> AccountHash {
        self.account_hash
    }

    pub fn balance(&self) -> Motes {
        self.balance
    }

    pub fn bonded_amount(&self) -> Motes {
        self.bonded_amount
    }
}

impl Distribution<GenesisAccount> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisAccount {
        let account_hash = AccountHash::new(rng.gen());

        let mut u512_array = [0u8; 64];
        rng.fill_bytes(u512_array.as_mut());
        let balance = Motes::new(U512::from(u512_array.as_ref()));

        rng.fill_bytes(u512_array.as_mut());
        let bonded_amount = Motes::new(U512::from(u512_array.as_ref()));

        GenesisAccount {
            account_hash,
            balance,
            bonded_amount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisConfig {
    name: String,
    timestamp: u64,
    protocol_version: ProtocolVersion,
    ee_config: ExecConfig,
}

impl GenesisConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        timestamp: u64,
        protocol_version: ProtocolVersion,
        ee_config: ExecConfig,
    ) -> Self {
        GenesisConfig {
            name,
            timestamp,
            protocol_version,
            ee_config,
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn ee_config(&self) -> &ExecConfig {
        &self.ee_config
    }

    pub fn ee_config_mut(&mut self) -> &mut ExecConfig {
        &mut self.ee_config
    }

    pub fn take_ee_config(self) -> ExecConfig {
        self.ee_config
    }
}

impl Distribution<GenesisConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisConfig {
        let count = rng.gen_range(1, 1000);
        let name = iter::repeat(())
            .map(|_| rng.gen::<char>())
            .take(count)
            .collect();

        let timestamp = rng.gen();

        let protocol_version = ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen());

        let ee_config = rng.gen();

        GenesisConfig {
            name,
            timestamp,
            protocol_version,
            ee_config,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecConfig {
    mint_installer_bytes: Vec<u8>,
    proof_of_stake_installer_bytes: Vec<u8>,
    standard_payment_installer_bytes: Vec<u8>,
    accounts: Vec<GenesisAccount>,
    wasm_costs: WasmCosts,
}

impl ExecConfig {
    pub fn new(
        mint_installer_bytes: Vec<u8>,
        proof_of_stake_installer_bytes: Vec<u8>,
        standard_payment_installer_bytes: Vec<u8>,
        accounts: Vec<GenesisAccount>,
        wasm_costs: WasmCosts,
    ) -> ExecConfig {
        ExecConfig {
            mint_installer_bytes,
            proof_of_stake_installer_bytes,
            standard_payment_installer_bytes,
            accounts,
            wasm_costs,
        }
    }
    pub fn mint_installer_bytes(&self) -> &[u8] {
        self.mint_installer_bytes.as_slice()
    }

    pub fn proof_of_stake_installer_bytes(&self) -> &[u8] {
        self.proof_of_stake_installer_bytes.as_slice()
    }

    pub fn standard_payment_installer_bytes(&self) -> &[u8] {
        self.standard_payment_installer_bytes.as_slice()
    }

    pub fn wasm_costs(&self) -> WasmCosts {
        self.wasm_costs
    }

    pub fn get_bonded_validators(&self) -> impl Iterator<Item = (AccountHash, Motes)> + '_ {
        let zero = Motes::zero();
        self.accounts.iter().filter_map(move |genesis_account| {
            if genesis_account.bonded_amount() > zero {
                Some((
                    genesis_account.account_hash(),
                    genesis_account.bonded_amount(),
                ))
            } else {
                None
            }
        })
    }

    pub fn accounts(&self) -> &[GenesisAccount] {
        self.accounts.as_slice()
    }

    pub fn push_account(&mut self, account: GenesisAccount) {
        self.accounts.push(account)
    }
}

impl Distribution<ExecConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ExecConfig {
        let mut count = rng.gen_range(1000, 10_000);
        let mint_installer_bytes = iter::repeat(()).map(|_| rng.gen()).take(count).collect();

        count = rng.gen_range(1000, 10_000);
        let proof_of_stake_installer_bytes =
            iter::repeat(()).map(|_| rng.gen()).take(count).collect();

        count = rng.gen_range(1000, 10_000);
        let standard_payment_installer_bytes =
            iter::repeat(()).map(|_| rng.gen()).take(count).collect();

        count = rng.gen_range(1, 10);
        let accounts = iter::repeat(()).map(|_| rng.gen()).take(count).collect();

        let wasm_costs = WasmCosts {
            regular: rng.gen(),
            div: rng.gen(),
            mul: rng.gen(),
            mem: rng.gen(),
            initial_mem: rng.gen(),
            grow_mem: rng.gen(),
            memcpy: rng.gen(),
            max_stack_height: rng.gen(),
            opcodes_mul: rng.gen(),
            opcodes_div: rng.gen(),
        };

        ExecConfig {
            mint_installer_bytes,
            proof_of_stake_installer_bytes,
            standard_payment_installer_bytes,
            accounts,
            wasm_costs,
        }
    }
}
