#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(any(feature = "json-schema", feature = "testing", test))]
use crate::{
    bytesrepr::{Bytes, ToBytes},
    transaction::transaction_v1::*,
};
#[cfg(any(feature = "testing", test))]
use crate::{PublicKey, RuntimeArgs, TransactionInvocationTarget, TransferTarget};
#[cfg(any(feature = "testing", test))]
use crate::{TransactionEntryPoint, TransactionScheduling, TransactionTarget};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::{AUCTION_LANE_ID, INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID};
#[cfg(any(feature = "json-schema", feature = "testing", test))]
use alloc::collections::BTreeMap;
#[cfg(any(feature = "testing", test))]
use rand::{Rng, RngCore};

#[cfg(any(feature = "json-schema", feature = "testing", feature = "gens", test))]
pub(crate) const ARGS_MAP_KEY: u16 = 0;
#[cfg(any(feature = "json-schema", feature = "testing", feature = "gens", test))]
pub(crate) const TARGET_MAP_KEY: u16 = 1;
#[cfg(any(feature = "json-schema", feature = "testing", feature = "gens", test))]
pub(crate) const ENTRY_POINT_MAP_KEY: u16 = 2;
#[cfg(any(feature = "json-schema", feature = "testing", feature = "gens", test))]
pub(crate) const SCHEDULING_MAP_KEY: u16 = 3;

#[cfg(any(feature = "testing", feature = "gens", test))]
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) enum FieldsContainerError {
    CouldNotSerializeField { field_index: u16 },
}

#[cfg(any(feature = "testing", feature = "gens", test))]
pub(crate) struct FieldsContainer {
    pub(super) args: TransactionArgs,
    pub(super) target: TransactionTarget,
    pub(super) entry_point: TransactionEntryPoint,
    pub(super) scheduling: TransactionScheduling,
}

#[cfg(any(feature = "testing", feature = "gens", test))]
impl FieldsContainer {
    pub(crate) fn new(
        args: TransactionArgs,
        target: TransactionTarget,
        entry_point: TransactionEntryPoint,
        scheduling: TransactionScheduling,
    ) -> Self {
        FieldsContainer {
            args,
            target,
            entry_point,
            scheduling,
        }
    }

    pub(crate) fn to_map(&self) -> Result<BTreeMap<u16, Bytes>, FieldsContainerError> {
        build_raw_payloads_map(
            &self.args,
            &self.target,
            &self.entry_point,
            &self.scheduling,
        )
        .map_err(|field_index| FieldsContainerError::CouldNotSerializeField { field_index })
    }

    /// Returns a random `FieldsContainer`.
    #[cfg(any(feature = "testing", test))]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..8) {
            0 => {
                let amount = rng.gen_range(2_500_000_000..=u64::MAX);
                let maybe_source = if rng.gen() { Some(rng.gen()) } else { None };
                let target = TransferTarget::random(rng);
                let maybe_id = rng.gen::<bool>().then(|| rng.gen());
                let args = arg_handling::new_transfer_args(amount, maybe_source, target, maybe_id)
                    .unwrap();
                FieldsContainer::new(
                    TransactionArgs::Named(args),
                    TransactionTarget::Native,
                    TransactionEntryPoint::Transfer,
                    TransactionScheduling::random(rng),
                )
            }
            1 => {
                let public_key = PublicKey::random(rng);
                let delegation_rate = rng.gen();
                let amount = rng.gen::<u64>();
                let minimum_delegation_amount = rng.gen::<bool>().then(|| rng.gen());
                let maximum_delegation_amount =
                    minimum_delegation_amount.map(|minimum_delegation_amount| {
                        minimum_delegation_amount + rng.gen::<u32>() as u64
                    });
                let reserved_slots = rng.gen::<bool>().then(|| rng.gen::<u32>());
                let args = arg_handling::new_add_bid_args(
                    public_key,
                    delegation_rate,
                    amount,
                    minimum_delegation_amount,
                    maximum_delegation_amount,
                    reserved_slots,
                )
                .unwrap();
                FieldsContainer::new(
                    TransactionArgs::Named(args),
                    TransactionTarget::Native,
                    TransactionEntryPoint::AddBid,
                    TransactionScheduling::random(rng),
                )
            }
            2 => {
                let public_key = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let args = arg_handling::new_withdraw_bid_args(public_key, amount).unwrap();
                FieldsContainer::new(
                    TransactionArgs::Named(args),
                    TransactionTarget::Native,
                    TransactionEntryPoint::WithdrawBid,
                    TransactionScheduling::random(rng),
                )
            }
            3 => {
                let delegator = PublicKey::random(rng);
                let validator = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let args = arg_handling::new_delegate_args(delegator, validator, amount).unwrap();
                FieldsContainer::new(
                    TransactionArgs::Named(args),
                    TransactionTarget::Native,
                    TransactionEntryPoint::Delegate,
                    TransactionScheduling::random(rng),
                )
            }
            4 => {
                let delegator = PublicKey::random(rng);
                let validator = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let args = arg_handling::new_undelegate_args(delegator, validator, amount).unwrap();
                FieldsContainer::new(
                    TransactionArgs::Named(args),
                    TransactionTarget::Native,
                    TransactionEntryPoint::Undelegate,
                    TransactionScheduling::random(rng),
                )
            }
            5 => {
                let delegator = PublicKey::random(rng);
                let validator = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let new_validator = PublicKey::random(rng);
                let args =
                    arg_handling::new_redelegate_args(delegator, validator, amount, new_validator)
                        .unwrap();
                FieldsContainer::new(
                    TransactionArgs::Named(args),
                    TransactionTarget::Native,
                    TransactionEntryPoint::Redelegate,
                    TransactionScheduling::random(rng),
                )
            }
            6 => Self::random_standard(rng),
            7 => {
                let mut buffer = vec![0u8; rng.gen_range(1..100)];
                rng.fill_bytes(buffer.as_mut());
                let is_install_upgrade = rng.gen();
                let target = TransactionTarget::Session {
                    is_install_upgrade,
                    module_bytes: Bytes::from(buffer),
                    runtime: crate::TransactionRuntimeParams::VmCasperV1,
                };
                FieldsContainer::new(
                    TransactionArgs::Named(RuntimeArgs::random(rng)),
                    target,
                    TransactionEntryPoint::Call,
                    TransactionScheduling::random(rng),
                )
            }
            _ => unreachable!(),
        }
    }

    /// Returns a random `FieldsContainer`.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_of_lane(rng: &mut TestRng, lane_id: u8) -> Self {
        match lane_id {
            MINT_LANE_ID => Self::random_transfer(rng),
            AUCTION_LANE_ID => Self::random_staking(rng),
            INSTALL_UPGRADE_LANE_ID => Self::random_install_upgrade(rng),
            _ => Self::random_standard(rng),
        }
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    fn random_transfer(rng: &mut TestRng) -> Self {
        let amount = rng.gen_range(2_500_000_000..=u64::MAX);
        let maybe_source = if rng.gen() { Some(rng.gen()) } else { None };
        let target = TransferTarget::random(rng);
        let maybe_id = rng.gen::<bool>().then(|| rng.gen());
        let args = arg_handling::new_transfer_args(amount, maybe_source, target, maybe_id).unwrap();
        FieldsContainer::new(
            TransactionArgs::Named(args),
            TransactionTarget::Native,
            TransactionEntryPoint::Transfer,
            TransactionScheduling::random(rng),
        )
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    fn random_install_upgrade(rng: &mut TestRng) -> Self {
        let target = TransactionTarget::Session {
            module_bytes: Bytes::from(rng.random_vec(0..100)),
            runtime: crate::TransactionRuntimeParams::VmCasperV1,
            is_install_upgrade: true,
        };
        FieldsContainer::new(
            TransactionArgs::Named(RuntimeArgs::random(rng)),
            target,
            TransactionEntryPoint::Call,
            TransactionScheduling::random(rng),
        )
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    fn random_staking(rng: &mut TestRng) -> Self {
        let public_key = PublicKey::random(rng);
        let delegation_rate = rng.gen();
        let amount = rng.gen::<u64>();
        let minimum_delegation_amount = rng.gen::<bool>().then(|| rng.gen());
        let maximum_delegation_amount = minimum_delegation_amount
            .map(|minimum_delegation_amount| minimum_delegation_amount + rng.gen::<u32>() as u64);
        let reserved_slots = rng.gen::<bool>().then(|| rng.gen::<u32>());
        let args = arg_handling::new_add_bid_args(
            public_key,
            delegation_rate,
            amount,
            minimum_delegation_amount,
            maximum_delegation_amount,
            reserved_slots,
        )
        .unwrap();
        FieldsContainer::new(
            TransactionArgs::Named(args),
            TransactionTarget::Native,
            TransactionEntryPoint::AddBid,
            TransactionScheduling::random(rng),
        )
    }

    #[cfg(any(feature = "testing", test))]
    fn random_standard(rng: &mut TestRng) -> Self {
        let target = TransactionTarget::Stored {
            id: TransactionInvocationTarget::random(rng),
            runtime: crate::transaction::transaction_target::TransactionRuntimeParams::VmCasperV1,
        };
        FieldsContainer::new(
            TransactionArgs::Named(RuntimeArgs::random(rng)),
            target,
            TransactionEntryPoint::Custom(rng.random_string(1..11)),
            TransactionScheduling::random(rng),
        )
    }
}

#[cfg(any(feature = "json-schema", feature = "testing", feature = "gens", test))]
pub(crate) fn build_raw_payloads_map(
    args: &TransactionArgs,
    target: &TransactionTarget,
    entry_point: &TransactionEntryPoint,
    scheduling: &TransactionScheduling,
) -> Result<BTreeMap<u16, Bytes>, u16> {
    let mut map: BTreeMap<u16, Bytes> = BTreeMap::new();
    map.insert(
        ARGS_MAP_KEY,
        args.to_bytes().map(Into::into).map_err(|_| ARGS_MAP_KEY)?,
    );
    map.insert(
        TARGET_MAP_KEY,
        target
            .to_bytes()
            .map(Into::into)
            .map_err(|_| TARGET_MAP_KEY)?,
    );
    map.insert(
        ENTRY_POINT_MAP_KEY,
        entry_point
            .to_bytes()
            .map(Into::into)
            .map_err(|_| ENTRY_POINT_MAP_KEY)?,
    );
    map.insert(
        SCHEDULING_MAP_KEY,
        scheduling
            .to_bytes()
            .map(Into::into)
            .map_err(|_| SCHEDULING_MAP_KEY)?,
    );
    Ok(map)
}
