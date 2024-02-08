mod error;

use core::marker::PhantomData;

#[cfg(any(feature = "testing", test))]
use rand::Rng;

use super::{
    super::{
        InitiatorAddr, TransactionEntryPoint, TransactionInvocationTarget, TransactionRuntime,
        TransactionScheduling, TransactionSessionKind, TransactionTarget,
    },
    transaction_v1_body::arg_handling,
    InitiatorAddrAndSecretKey, PricingMode, TransactionV1, TransactionV1Body,
};
use crate::{
    account::AccountHash, bytesrepr::Bytes, CLValue, CLValueError, EntityAddr, EntityVersion,
    PackageAddr, PublicKey, RuntimeArgs, SecretKey, TimeDiff, Timestamp, URef, U512,
};
#[cfg(any(feature = "testing", test))]
use crate::{testing::TestRng, TransactionConfig, TransactionV1Approval, TransactionV1Hash};
pub use error::TransactionV1BuilderError;

/// A builder for constructing a [`TransactionV1`].
///
/// # Note
///
/// Before calling [`build`](Self::build), you must ensure that:
///   * an initiator_addr is provided by either calling
///     [`with_initiator_addr`](Self::with_initiator_addr) or
///     [`with_secret_key`](Self::with_secret_key)
///   * the chain name is set by calling [`with_chain_name`](Self::with_chain_name)
///
/// If no secret key is provided, the resulting transaction will be unsigned, and hence invalid.
/// It can be signed later (multiple times if desired) to make it valid before sending to the
/// network for execution.
pub struct TransactionV1Builder<'a> {
    chain_name: Option<String>,
    timestamp: Timestamp,
    ttl: TimeDiff,
    body: TransactionV1Body,
    pricing_mode: PricingMode,
    payment_amount: Option<u64>,
    initiator_addr: Option<InitiatorAddr>,
    #[cfg(not(any(feature = "testing", test)))]
    secret_key: Option<&'a SecretKey>,
    #[cfg(any(feature = "testing", test))]
    secret_key: Option<SecretKey>,
    #[cfg(any(feature = "testing", test))]
    invalid_approvals: Vec<TransactionV1Approval>,
    _phantom_data: PhantomData<&'a ()>,
}

impl<'a> TransactionV1Builder<'a> {
    /// The default time-to-live for transactions, i.e. 30 minutes.
    pub const DEFAULT_TTL: TimeDiff = TimeDiff::from_millis(30 * 60 * 1_000);
    /// The default pricing mode for transactions, i.e. multiplier of 1.
    pub const DEFAULT_PRICING_MODE: PricingMode = PricingMode::GasPriceMultiplier(1);
    /// The default runtime for transactions, i.e. Casper Version 1 Virtual Machine.
    pub const DEFAULT_RUNTIME: TransactionRuntime = TransactionRuntime::VmCasperV1;
    /// The default scheduling for transactions, i.e. `Standard`.
    pub const DEFAULT_SCHEDULING: TransactionScheduling = TransactionScheduling::Standard;

    fn new(body: TransactionV1Body) -> Self {
        TransactionV1Builder {
            chain_name: None,
            timestamp: Timestamp::now(),
            ttl: Self::DEFAULT_TTL,
            body,
            pricing_mode: Self::DEFAULT_PRICING_MODE,
            payment_amount: None,
            initiator_addr: None,
            secret_key: None,
            _phantom_data: PhantomData,
            #[cfg(any(feature = "testing", test))]
            invalid_approvals: vec![],
        }
    }

    /// Returns a new `TransactionV1Builder` suitable for building a native transfer transaction.
    pub fn new_transfer<A: Into<U512>>(
        source: URef,
        target: URef,
        amount: A,
        maybe_to: Option<AccountHash>,
        maybe_id: Option<u64>,
    ) -> Result<Self, CLValueError> {
        let args = arg_handling::new_transfer_args(source, target, amount, maybe_to, maybe_id)?;
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::Transfer,
            Self::DEFAULT_SCHEDULING,
        );
        Ok(TransactionV1Builder::new(body))
    }

    /// Returns a new `TransactionV1Builder` suitable for building a native add_bid transaction.
    pub fn new_add_bid<A: Into<U512>>(
        public_key: PublicKey,
        delegation_rate: u8,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let args = arg_handling::new_add_bid_args(public_key, delegation_rate, amount)?;
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::AddBid,
            Self::DEFAULT_SCHEDULING,
        );
        Ok(TransactionV1Builder::new(body))
    }

    /// Returns a new `TransactionV1Builder` suitable for building a native withdraw_bid
    /// transaction.
    pub fn new_withdraw_bid<A: Into<U512>>(
        public_key: PublicKey,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let args = arg_handling::new_withdraw_bid_args(public_key, amount)?;
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::WithdrawBid,
            Self::DEFAULT_SCHEDULING,
        );
        Ok(TransactionV1Builder::new(body))
    }

    /// Returns a new `TransactionV1Builder` suitable for building a native delegate transaction.
    pub fn new_delegate<A: Into<U512>>(
        delegator: PublicKey,
        validator: PublicKey,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let args = arg_handling::new_delegate_args(delegator, validator, amount)?;
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::Delegate,
            Self::DEFAULT_SCHEDULING,
        );
        Ok(TransactionV1Builder::new(body))
    }

    /// Returns a new `TransactionV1Builder` suitable for building a native undelegate transaction.
    pub fn new_undelegate<A: Into<U512>>(
        delegator: PublicKey,
        validator: PublicKey,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let args = arg_handling::new_undelegate_args(delegator, validator, amount)?;
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::Undelegate,
            Self::DEFAULT_SCHEDULING,
        );
        Ok(TransactionV1Builder::new(body))
    }

    /// Returns a new `TransactionV1Builder` suitable for building a native redelegate transaction.
    pub fn new_redelegate<A: Into<U512>>(
        delegator: PublicKey,
        validator: PublicKey,
        amount: A,
        new_validator: PublicKey,
    ) -> Result<Self, CLValueError> {
        let args = arg_handling::new_redelegate_args(delegator, validator, amount, new_validator)?;
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::Redelegate,
            Self::DEFAULT_SCHEDULING,
        );
        Ok(TransactionV1Builder::new(body))
    }

    fn new_targeting_stored<E: Into<String>>(
        id: TransactionInvocationTarget,
        entry_point: E,
    ) -> Self {
        let target = TransactionTarget::Stored {
            id,
            runtime: Self::DEFAULT_RUNTIME,
        };
        let body = TransactionV1Body::new(
            RuntimeArgs::new(),
            target,
            TransactionEntryPoint::Custom(entry_point.into()),
            Self::DEFAULT_SCHEDULING,
        );
        TransactionV1Builder::new(body)
    }

    /// Returns a new `TransactionV1Builder` suitable for building a transaction targeting a stored
    /// entity.
    pub fn new_targeting_invocable_entity<E: Into<String>>(
        addr: EntityAddr,
        entry_point: E,
    ) -> Self {
        let id = TransactionInvocationTarget::new_invocable_entity(addr);
        Self::new_targeting_stored(id, entry_point)
    }

    /// Returns a new `TransactionV1Builder` suitable for building a transaction targeting a stored
    /// entity via its alias.
    pub fn new_targeting_invocable_entity_via_alias<A: Into<String>, E: Into<String>>(
        alias: A,
        entry_point: E,
    ) -> Self {
        let id = TransactionInvocationTarget::new_invocable_entity_alias(alias.into());
        Self::new_targeting_stored(id, entry_point)
    }

    /// Returns a new `TransactionV1Builder` suitable for building a transaction targeting a
    /// package.
    pub fn new_targeting_package<E: Into<String>>(
        addr: PackageAddr,
        version: Option<EntityVersion>,
        entry_point: E,
    ) -> Self {
        let id = TransactionInvocationTarget::new_package(addr, version);
        Self::new_targeting_stored(id, entry_point)
    }

    /// Returns a new `TransactionV1Builder` suitable for building a transaction targeting a
    /// package via its alias.
    pub fn new_targeting_package_via_alias<A: Into<String>, E: Into<String>>(
        alias: A,
        version: Option<EntityVersion>,
        entry_point: E,
    ) -> Self {
        let id = TransactionInvocationTarget::new_package_alias(alias.into(), version);
        Self::new_targeting_stored(id, entry_point)
    }

    /// Returns a new `TransactionV1Builder` suitable for building a transaction for running session
    /// logic, i.e. compiled Wasm.
    pub fn new_session<E: Into<String>>(
        kind: TransactionSessionKind,
        module_bytes: Bytes,
        entry_point: E,
    ) -> Self {
        let target = TransactionTarget::Session {
            kind,
            module_bytes,
            runtime: Self::DEFAULT_RUNTIME,
        };
        let body = TransactionV1Body::new(
            RuntimeArgs::new(),
            target,
            TransactionEntryPoint::Custom(entry_point.into()),
            Self::DEFAULT_SCHEDULING,
        );
        TransactionV1Builder::new(body)
    }

    /// Returns a new `TransactionV1Builder` which will build a random, valid but possibly expired
    /// transaction.
    ///
    /// The transaction can be made invalid in the following ways:
    ///   * unsigned by calling `with_no_secret_key`
    ///   * given an invalid approval by calling `with_invalid_approval`
    #[cfg(any(feature = "testing", test))]
    pub fn new_random(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let ttl_millis = rng.gen_range(60_000..TransactionConfig::default().max_ttl.millis());
        let body = TransactionV1Body::random(rng);
        TransactionV1Builder {
            chain_name: Some(rng.random_string(5..10)),
            timestamp: Timestamp::random(rng),
            ttl: TimeDiff::from_millis(ttl_millis),
            body,
            pricing_mode: PricingMode::random(rng),
            payment_amount: Some(
                rng.gen_range(2_500_000_000..=TransactionConfig::default().block_gas_limit),
            ),
            initiator_addr: Some(InitiatorAddr::PublicKey(PublicKey::from(&secret_key))),
            secret_key: Some(secret_key),
            _phantom_data: PhantomData,
            invalid_approvals: vec![],
        }
    }

    /// Sets the `chain_name` in the transaction.
    ///
    /// Must be provided or building will fail.
    pub fn with_chain_name<C: Into<String>>(mut self, chain_name: C) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Sets the `timestamp` in the transaction.
    ///
    /// If not provided, the timestamp will be set to the time when the builder was constructed.
    pub fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Sets the `ttl` (time-to-live) in the transaction.
    ///
    /// If not provided, the ttl will be set to [`Self::DEFAULT_TTL`].
    pub fn with_ttl(mut self, ttl: TimeDiff) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the `pricing_mode` in the transaction.
    ///
    /// If not provided, the pricing mode will be set to [`Self::DEFAULT_PRICING_MODE`].
    pub fn with_pricing_mode(mut self, pricing_mode: PricingMode) -> Self {
        self.pricing_mode = pricing_mode;
        self
    }

    /// Sets the `payment_amount` in the transaction.
    ///
    /// If not provided, `payment_amount` will be set to `None`.
    pub fn with_payment_amount(mut self, payment_amount: u64) -> Self {
        self.payment_amount = Some(payment_amount);
        self
    }

    /// Sets the `initiator_addr` in the transaction.
    ///
    /// If not provided, the public key derived from the secret key used in the builder will be
    /// used as the `InitiatorAddr::PublicKey` in the transaction.
    pub fn with_initiator_addr(mut self, initiator_addr: InitiatorAddr) -> Self {
        self.initiator_addr = Some(initiator_addr);
        self
    }

    /// Sets the secret key used to sign the transaction on calling [`build`](Self::build).
    ///
    /// If not provided, the transaction can still be built, but will be unsigned and will be
    /// invalid until subsequently signed.
    pub fn with_secret_key(mut self, secret_key: &'a SecretKey) -> Self {
        #[cfg(not(any(feature = "testing", test)))]
        {
            self.secret_key = Some(secret_key);
        }
        #[cfg(any(feature = "testing", test))]
        {
            self.secret_key = Some(
                SecretKey::from_der(secret_key.to_der().expect("should der-encode"))
                    .expect("should der-decode"),
            );
        }
        self
    }

    /// Appends the given runtime arg into the body's `args`.
    pub fn with_runtime_arg<K: Into<String>>(mut self, key: K, cl_value: CLValue) -> Self {
        self.body.args.insert_cl_value(key, cl_value);
        self
    }

    /// Sets the runtime args in the transaction.
    ///
    /// NOTE: this overwrites any existing runtime args.  To append to existing args, use
    /// [`TransactionV1Builder::with_runtime_arg`].
    pub fn with_runtime_args(mut self, args: RuntimeArgs) -> Self {
        self.body.args = args;
        self
    }

    /// Sets the runtime for the transaction.
    ///
    /// If not provided, the runtime will be set to [`Self::DEFAULT_RUNTIME`].
    ///
    /// NOTE: This has no effect for native transactions, i.e. where the `body.target` is
    /// `TransactionTarget::Native`.
    pub fn with_runtime(mut self, runtime: TransactionRuntime) -> Self {
        match &mut self.body.target {
            TransactionTarget::Native => {}
            TransactionTarget::Stored {
                runtime: existing_runtime,
                ..
            } => {
                *existing_runtime = runtime;
            }
            TransactionTarget::Session {
                runtime: existing_runtime,
                ..
            } => {
                *existing_runtime = runtime;
            }
        }
        self
    }

    /// Sets the scheduling for the transaction.
    ///
    /// If not provided, the scheduling will be set to [`Self::DEFAULT_SCHEDULING`].
    pub fn with_scheduling(mut self, scheduling: TransactionScheduling) -> Self {
        self.body.scheduling = scheduling;
        self
    }

    /// Sets the secret key to `None`, meaning the transaction can still be built but will be
    /// unsigned and will be invalid until subsequently signed.
    #[cfg(any(feature = "testing", test))]
    pub fn with_no_secret_key(mut self) -> Self {
        self.secret_key = None;
        self
    }

    /// Sets an invalid approval in the transaction.
    #[cfg(any(feature = "testing", test))]
    pub fn with_invalid_approval(mut self, rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let hash = TransactionV1Hash::random(rng);
        let approval = TransactionV1Approval::create(&hash, &secret_key);
        self.invalid_approvals.push(approval);
        self
    }

    /// Returns the new transaction, or an error if non-defaulted fields were not set.
    ///
    /// For more info, see [the `TransactionBuilder` documentation](TransactionV1Builder).
    pub fn build(self) -> Result<TransactionV1, TransactionV1BuilderError> {
        self.do_build()
    }

    #[cfg(not(any(feature = "testing", test)))]
    fn do_build(self) -> Result<TransactionV1, TransactionV1BuilderError> {
        let initiator_addr_and_secret_key = match (self.initiator_addr, self.secret_key) {
            (Some(initiator_addr), Some(secret_key)) => InitiatorAddrAndSecretKey::Both {
                initiator_addr,
                secret_key,
            },
            (Some(initiator_addr), None) => {
                InitiatorAddrAndSecretKey::InitiatorAddr(initiator_addr)
            }
            (None, Some(secret_key)) => InitiatorAddrAndSecretKey::SecretKey(secret_key),
            (None, None) => return Err(TransactionV1BuilderError::MissingInitiatorAddr),
        };

        let chain_name = self
            .chain_name
            .ok_or(TransactionV1BuilderError::MissingChainName)?;

        let transaction = TransactionV1::build(
            chain_name,
            self.timestamp,
            self.ttl,
            self.body,
            self.pricing_mode,
            self.payment_amount,
            initiator_addr_and_secret_key,
        );

        Ok(transaction)
    }

    #[cfg(any(feature = "testing", test))]
    fn do_build(self) -> Result<TransactionV1, TransactionV1BuilderError> {
        let initiator_addr_and_secret_key = match (self.initiator_addr, &self.secret_key) {
            (Some(initiator_addr), Some(secret_key)) => InitiatorAddrAndSecretKey::Both {
                initiator_addr,
                secret_key,
            },
            (Some(initiator_addr), None) => {
                InitiatorAddrAndSecretKey::InitiatorAddr(initiator_addr)
            }
            (None, Some(secret_key)) => InitiatorAddrAndSecretKey::SecretKey(secret_key),
            (None, None) => return Err(TransactionV1BuilderError::MissingInitiatorAddr),
        };

        let chain_name = self
            .chain_name
            .ok_or(TransactionV1BuilderError::MissingChainName)?;

        let mut transaction = TransactionV1::build(
            chain_name,
            self.timestamp,
            self.ttl,
            self.body,
            self.pricing_mode,
            self.payment_amount,
            initiator_addr_and_secret_key,
        );

        transaction.apply_approvals(self.invalid_approvals);

        Ok(transaction)
    }
}
