use std::{
    collections::BTreeMap,
    mem,
    time::{Duration, Instant},
};

use crate::{
    core::resolvers::v1_function_index::FunctionIndex, shared::logging::log_host_function_metrics,
};

enum PauseState {
    NotStarted,
    Started(Instant),
    Completed(Duration),
}

impl PauseState {
    fn new() -> Self {
        PauseState::NotStarted
    }

    fn activate(&mut self) {
        match self {
            PauseState::NotStarted => {
                *self = PauseState::Started(Instant::now());
            }
            _ => panic!("PauseState must be NotStarted"),
        }
    }

    fn complete(&mut self) {
        match self {
            PauseState::Started(start) => {
                *self = PauseState::Completed(start.elapsed());
            }
            _ => panic!("Pause must already be active"),
        }
    }

    fn duration(&self) -> Duration {
        match self {
            PauseState::NotStarted => Duration::default(),
            PauseState::Completed(duration) => *duration,
            PauseState::Started(start) => start.elapsed(),
        }
    }
}

pub(super) struct ScopedInstrumenter {
    start: Instant,
    pause_state: PauseState,
    function_index: FunctionIndex,
    properties: BTreeMap<&'static str, String>,
}

impl ScopedInstrumenter {
    pub(super) fn new(function_index: FunctionIndex) -> Self {
        ScopedInstrumenter {
            start: Instant::now(),
            pause_state: PauseState::new(),
            function_index,
            properties: BTreeMap::new(),
        }
    }

    pub(super) fn add_property<T: ToString>(&mut self, key: &'static str, value: T) {
        assert!(self.properties.insert(key, value.to_string()).is_none());
    }

    /// Can be called once only to effectively pause the running timer.  `unpause` can likewise be
    /// called once if the timer has already been paused.
    pub(super) fn pause(&mut self) {
        self.pause_state.activate();
    }

    pub(super) fn unpause(&mut self) {
        self.pause_state.complete();
    }

    fn duration(&self) -> Duration {
        self.start
            .elapsed()
            .checked_sub(self.pause_state.duration())
            .unwrap_or_default()
    }
}

impl Drop for ScopedInstrumenter {
    fn drop(&mut self) {
        let duration = self.duration();
        let host_function = match self.function_index {
            FunctionIndex::GasFuncIndex => return,
            FunctionIndex::WriteFuncIndex => "host_function_write",
            FunctionIndex::ReadFuncIndex => "host_function_read_value",
            FunctionIndex::AddFuncIndex => "host_function_add",
            FunctionIndex::NewFuncIndex => "host_function_new_uref",
            FunctionIndex::RetFuncIndex => "host_function_ret",
            FunctionIndex::CallContractFuncIndex => "host_function_call_contract",
            FunctionIndex::GetKeyFuncIndex => "host_function_get_key",
            FunctionIndex::HasKeyFuncIndex => "host_function_has_key",
            FunctionIndex::PutKeyFuncIndex => "host_function_put_key",
            FunctionIndex::IsValidURefFnIndex => "host_function_is_valid_uref",
            FunctionIndex::RevertFuncIndex => "host_function_revert",
            FunctionIndex::AddAssociatedKeyFuncIndex => "host_function_add_associated_key",
            FunctionIndex::RemoveAssociatedKeyFuncIndex => "host_function_remove_associated_key",
            FunctionIndex::UpdateAssociatedKeyFuncIndex => "host_function_update_associated_key",
            FunctionIndex::SetActionThresholdFuncIndex => "host_function_set_action_threshold",
            FunctionIndex::LoadNamedKeysFuncIndex => "host_function_load_named_keys",
            FunctionIndex::RemoveKeyFuncIndex => "host_function_remove_key",
            FunctionIndex::GetCallerIndex => "host_function_get_caller",
            FunctionIndex::GetBlocktimeIndex => "host_function_get_blocktime",
            FunctionIndex::CreatePurseIndex => "host_function_create_purse",
            FunctionIndex::TransferToAccountIndex => "host_function_transfer_to_account",
            FunctionIndex::TransferFromPurseToAccountIndex => {
                "host_function_transfer_from_purse_to_account"
            }
            FunctionIndex::TransferFromPurseToPurseIndex => {
                "host_function_transfer_from_purse_to_purse"
            }
            FunctionIndex::GetBalanceIndex => "host_function_get_balance",
            FunctionIndex::GetPhaseIndex => "host_function_get_phase",
            FunctionIndex::GetMainPurseIndex => "host_function_get_main_purse",
            FunctionIndex::ReadHostBufferIndex => "host_function_read_host_buffer",
            FunctionIndex::CreateContractPackageAtHash => {
                "host_function_create_contract_package_at_hash"
            }
            FunctionIndex::AddContractVersion => "host_function_add_contract_version",
            FunctionIndex::DisableContractVersion => "host_remove_contract_version",
            FunctionIndex::CallVersionedContract => "host_call_versioned_contract",
            FunctionIndex::CreateContractUserGroup => "create_contract_user_group",
            #[cfg(feature = "test-support")]
            FunctionIndex::PrintIndex => "host_function_print",
            FunctionIndex::GetRuntimeArgsizeIndex => "host_get_named_arg_size",
            FunctionIndex::GetRuntimeArgIndex => "host_get_named_arg",
            FunctionIndex::RemoveContractUserGroupIndex => "host_remove_contract_user_group",
            FunctionIndex::ExtendContractUserGroupURefsIndex => {
                "host_provision_contract_user_group_uref"
            }
            FunctionIndex::RemoveContractUserGroupURefsIndex => {
                "host_remove_contract_user_group_urefs"
            }
            FunctionIndex::Blake2b => "host_blake2b",
            FunctionIndex::RecordTransfer => "host_record_transfer",
            FunctionIndex::RecordEraInfo => "host_record_era_info",
        };

        let mut properties = mem::take(&mut self.properties);
        properties.insert(
            "duration_in_seconds",
            format!("{:.06e}", duration.as_secs_f64()),
        );

        log_host_function_metrics(host_function, properties);
    }
}
