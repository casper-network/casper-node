use super::*;

pub(super) struct TestChainBuilder {
    nodes: Nodes,
    keys: Option<Vec<Arc<SecretKey>>>,
    #[allow(clippy::type_complexity)]
    chainspec_update: Option<Box<dyn Fn(&mut Chainspec)>>,
}

enum Nodes {
    Random(usize),
    RandomWithStake(Vec<U512>),
    Stake(BTreeMap<Arc<SecretKey>, U512>),
}

impl Nodes {
    fn get(self, rng: &mut TestRng) -> BTreeMap<Arc<SecretKey>, U512> {
        match self {
            Self::Random(size) => {
                (0..size)
                    .map(|_| {
                        // By default we use very large stakes so we would catch overflow issues.
                        let private_key = Arc::new(SecretKey::random(rng));
                        let amount = U512::from(rng.gen_range(100..999)) * U512::from(u128::MAX);

                        (private_key, amount)
                    })
                    .collect()
            }
            Self::RandomWithStake(stake) => stake
                .into_iter()
                .map(|amount| (Arc::new(SecretKey::random(rng)), amount))
                .collect(),
            Self::Stake(stake) => stake,
        }
    }
}

impl TestChainBuilder {
    fn new() -> Self {
        Self {
            nodes: Nodes::Random(0),
            keys: None,
            chainspec_update: None,
        }
    }

    pub(super) fn new_with_size(size: usize) -> Self {
        Self {
            nodes: Nodes::Random(size),
            ..Self::new()
        }
    }

    #[allow(unused)]
    pub(super) fn new_with_stakes(initial_stakes: Vec<U512>) -> Self {
        Self {
            nodes: Nodes::RandomWithStake(initial_stakes),
            ..Self::new()
        }
    }

    #[allow(unused)]
    pub(super) fn new_with_keys(initial_stakes: BTreeMap<Arc<SecretKey>, U512>) -> Self {
        Self {
            nodes: Nodes::Stake(initial_stakes),
            ..Self::new()
        }
    }

    /// Use custom validator public keys which may be different from the one
    /// used for the stake (for creating an equivocation).
    #[allow(unused)]
    pub(super) fn keys(self, keys: Vec<Arc<SecretKey>>) -> Self {
        Self {
            keys: Some(keys),
            ..self
        }
    }
    pub(super) fn chainspec(self, chainspec_update: impl Fn(&mut Chainspec) + 'static) -> Self {
        Self {
            chainspec_update: Some(Box::new(chainspec_update)),
            ..self
        }
    }

    pub(super) async fn build(
        self,
        rng: &mut TestRng,
    ) -> (TestChain, TestingNetwork<FilterReactor<MainReactor>>) {
        let Self {
            nodes,
            keys,
            chainspec_update,
        } = self;

        let stakes = nodes.get(rng);

        let (chainspec, chainspec_raw_bytes) = {
            // Load the `local` chainspec.
            let (mut chainspec, chainspec_raw_bytes) =
                <(Chainspec, ChainspecRawBytes)>::from_resources("local");

            let min_motes = 100_000_000_000u64; // 1000 token
            let max_motes = min_motes * 100; // 100_000 token
            let balance = U512::from(rng.gen_range(min_motes..max_motes));

            // Override accounts with those generated from the keys.
            let accounts = stakes
                .iter()
                .map(|(secret_key, bonded_amount)| {
                    let public_key = PublicKey::from(secret_key.as_ref());
                    let validator_config =
                        ValidatorConfig::new(Motes::new(*bonded_amount), DelegationRate::zero());
                    AccountConfig::new(public_key, Motes::new(balance), Some(validator_config))
                })
                .collect();

            let delegators = vec![];
            let administrators = vec![];
            chainspec.network_config.accounts_config =
                AccountsConfig::new(accounts, delegators, administrators);

            // Make the genesis timestamp 60 seconds from now, to allow for all validators to start
            // up.
            let genesis_time = Timestamp::now() + TimeDiff::from_seconds(60);
            info!(
                "creating test chain configuration, genesis: {}",
                genesis_time
            );
            chainspec.protocol_config.activation_point = ActivationPoint::Genesis(genesis_time);

            chainspec.core_config.minimum_era_height = 1;
            chainspec.core_config.finality_threshold_fraction = Ratio::new(34, 100);
            chainspec.core_config.era_duration = TimeDiff::from_millis(10);
            chainspec.core_config.auction_delay = 1;
            chainspec.core_config.unbonding_delay = 3;

            if let Some(chainspec_update) = chainspec_update {
                chainspec_update(&mut chainspec);
            }

            (chainspec, chainspec_raw_bytes)
        };

        let first_node_port = testing::unused_port_on_localhost();

        let keys = keys.unwrap_or_else(|| stakes.into_iter().map(|pair| pair.0).collect());

        let mut chain = TestChain {
            keys,
            storages: Vec::new(),
            chainspec: Arc::new(chainspec),
            chainspec_raw_bytes: Arc::new(chainspec_raw_bytes),
            first_node_port,
        };

        let net = chain
            .create_initialized_network(rng)
            .await
            .expect("network initialization failed");

        (chain, net)
    }
}
