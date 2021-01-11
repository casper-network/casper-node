use std::convert::{TryFrom, TryInto};

use num_rational::Ratio;

use casper_execution_engine::core::engine_state::upgrade::UpgradeConfig;
use casper_types::{auction::EraId, ProtocolVersion};

use crate::engine_server::{ipc::UpgradeRequest, mappings::MappingError};

impl TryFrom<UpgradeRequest> for UpgradeConfig {
    type Error = MappingError;

    fn try_from(mut pb_upgrade_request: UpgradeRequest) -> Result<Self, Self::Error> {
        let pre_state_hash = pb_upgrade_request
            .get_parent_state_hash()
            .try_into()
            .map_err(|_| MappingError::InvalidStateHash("pre_state_hash".to_string()))?;

        let current_protocol_version = pb_upgrade_request.take_protocol_version().into();

        let upgrade_point = pb_upgrade_request.mut_upgrade_point();
        let new_protocol_version: ProtocolVersion = upgrade_point.take_protocol_version().into();

        let wasm_config = if !upgrade_point.has_new_wasm_config() {
            None
        } else {
            Some(upgrade_point.take_new_wasm_config().try_into()?)
        };
        let activation_point = if !upgrade_point.has_activation_point() {
            None
        } else {
            Some(upgrade_point.get_activation_point().height)
        };

        let new_validator_slots: Option<u32> = if !upgrade_point.has_new_validator_slots() {
            None
        } else {
            Some(
                upgrade_point
                    .take_new_validator_slots()
                    .get_new_validator_slots(),
            )
        };

        let new_auction_delay: Option<u64> = if !upgrade_point.has_new_auction_delay() {
            None
        } else {
            Some(
                upgrade_point
                    .take_new_auction_delay()
                    .get_new_auction_delay(),
            )
        };

        let new_locked_funds_period: Option<EraId> = if !upgrade_point.has_new_locked_funds_period()
        {
            None
        } else {
            Some(
                upgrade_point
                    .take_new_locked_funds_period()
                    .get_new_locked_funds_period(),
            )
        };

        let new_round_seigniorage_rate: Option<Ratio<u64>> = upgrade_point
            .new_round_seigniorage_rate
            .as_ref()
            .cloned()
            .map(Into::into);

        let new_unbonding_delay: Option<EraId> = if !upgrade_point.has_new_unbonding_delay() {
            None
        } else {
            Some(
                upgrade_point
                    .take_new_unbonding_delay()
                    .get_new_unbonding_delay(),
            )
        };

        let new_wasmless_transfer_cost: Option<u64> =
            if !upgrade_point.has_new_wasmless_transfer_cost() {
                None
            } else {
                Some(
                    upgrade_point
                        .take_new_wasmless_transfer_cost()
                        .get_new_wasmless_transfer_cost(),
                )
            };

        Ok(UpgradeConfig::new(
            pre_state_hash,
            current_protocol_version,
            new_protocol_version,
            wasm_config,
            activation_point,
            new_validator_slots,
            new_auction_delay,
            new_locked_funds_period,
            new_round_seigniorage_rate,
            new_unbonding_delay,
            new_wasmless_transfer_cost,
        ))
    }
}
