use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::bail;
use either::Either;
use log::info;
use num::Zero;
use num_rational::Ratio;
use rand::Rng;
use tempfile::TempDir;
use tokio::time;

use casper_execution_engine::{core::engine_state::query::GetBidsRequest, shared::motes::Motes};
use casper_types::{
    system::auction::{Bids, DelegationRate},
    EraId, PublicKey, SecretKey, U512,
};

use crate::{
    components::{consensus, gossiper, small_network, storage},
    crypto::AsymmetricKeyExt,
    effect::EffectExt,
    reactor::{
        initializer, joiner,
        participating::{self, ParticipatingEvent},
        ReactorExit, Runner,
    },
    testing::{
        self, filter_reactor::FilterReactor, network::Network, ConditionCheckReactor, TestRng,
    },
    types::{
        chainspec::{AccountConfig, AccountsConfig, ValidatorConfig},
        ActivationPoint, BlockHeader, Chainspec, Timestamp,
    },
    utils::{External, Loadable, WithDir, RESOURCES_PATH},
    NodeRng,
};

struct TestChain {
    // Keys that validator instances will use, can include duplicates
    keys: Vec<Arc<SecretKey>>,
    storages: Vec<TempDir>,
    chainspec: Arc<Chainspec>,
}

type Nodes = crate::testing::network::Nodes<FilterReactor<participating::Reactor>>;

impl Runner<ConditionCheckReactor<FilterReactor<participating::Reactor>>> {
    fn participating(&self) -> &participating::Reactor {
        self.reactor().inner().inner()
    }
}

impl TestChain {
    /// Instantiates a new test chain configuration.
    ///
    /// Generates secret keys for `size` validators and creates a matching chainspec.
    fn new(rng: &mut TestRng, size: usize) -> Self {
        let keys: Vec<Arc<SecretKey>> = (0..size)
            .map(|_| Arc::new(SecretKey::random(rng)))
            .collect();
        let stakes = keys
            .iter()
            .map(|secret_key| {
                // We use very large stakes so we would catch overflow issues.
                let stake = U512::from(rng.gen_range(100..999)) * U512::from(u128::MAX);
                let secret_key = secret_key.clone();
                (PublicKey::from(&*secret_key), stake)
            })
            .collect();
        Self::new_with_keys(rng, keys, stakes)
    }

    /// Instantiates a new test chain configuration.
    ///
    /// Takes a vector of bonded keys with specified bond amounts.
    fn new_with_keys(
        rng: &mut TestRng,
        keys: Vec<Arc<SecretKey>>,
        stakes: BTreeMap<PublicKey, U512>,
    ) -> Self {
        // Load the `local` chainspec.
        let mut chainspec = Chainspec::from_resources("local");

        // Override accounts with those generated from the keys.
        let accounts = stakes
            .into_iter()
            .map(|(public_key, bonded_amount)| {
                let validator_config =
                    ValidatorConfig::new(Motes::new(bonded_amount), DelegationRate::zero());
                AccountConfig::new(
                    public_key,
                    Motes::new(U512::from(rng.gen_range(10000..99999999))),
                    Some(validator_config),
                )
            })
            .collect();
        let delegators = vec![];
        chainspec.network_config.accounts_config = AccountsConfig::new(accounts, delegators);

        // Make the genesis timestamp 45 seconds from now, to allow for all validators to start up.
        chainspec.protocol_config.activation_point =
            ActivationPoint::Genesis(Timestamp::now() + 45000.into());

        chainspec.core_config.minimum_era_height = 1;
        chainspec.highway_config.finality_threshold_fraction = Ratio::new(34, 100);
        chainspec.core_config.era_duration = 10.into();
        chainspec.core_config.auction_delay = 1;
        chainspec.core_config.unbonding_delay = 3;

        TestChain {
            keys,
            chainspec: Arc::new(chainspec),
            storages: Vec::new(),
        }
    }

    fn chainspec_mut(&mut self) -> &mut Chainspec {
        Arc::get_mut(&mut self.chainspec).unwrap()
    }

    /// Creates an initializer/validator configuration for the `idx`th validator.
    fn create_node_config(&mut self, idx: usize, first_node_port: u16) -> participating::Config {
        // Set the network configuration.
        let mut cfg = participating::Config {
            network: if idx == 0 {
                small_network::Config::default_local_net_first_node(first_node_port)
            } else {
                small_network::Config::default_local_net(first_node_port)
            },
            gossip: gossiper::Config::new_with_small_timeouts(),
            ..Default::default()
        };

        // Additionally set up storage in a temporary directory.
        let (storage_cfg, temp_dir) = storage::Config::default_for_tests();
        // ...and the secret key for our validator.
        {
            let secret_key_path = temp_dir.path().join("secret_key");
            self.keys[idx]
                .to_file(secret_key_path.clone())
                .expect("could not write secret key");
            cfg.consensus.secret_key_path = External::Path(secret_key_path);
        }
        cfg.consensus.highway.unit_hashes_folder = temp_dir.path().to_path_buf();
        self.storages.push(temp_dir);
        cfg.storage = storage_cfg;

        cfg
    }

    async fn create_initialized_network(
        &mut self,
        rng: &mut NodeRng,
    ) -> anyhow::Result<Network<FilterReactor<participating::Reactor>>> {
        let root = RESOURCES_PATH.join("local");

        let mut network: Network<FilterReactor<participating::Reactor>> = Network::new();
        let first_node_port = testing::unused_port_on_localhost();

        for idx in 0..self.keys.len() {
            let cfg = self.create_node_config(idx, first_node_port);

            // We create an initializer reactor here and run it to completion.
            let mut initializer_runner = Runner::<initializer::Reactor>::new_with_chainspec(
                (false, WithDir::new(root.clone(), cfg)),
                Arc::clone(&self.chainspec),
            )
            .await?;
            let reactor_exit = initializer_runner.run(rng).await;
            if reactor_exit != ReactorExit::ProcessShouldContinue {
                bail!("failed to initialize successfully");
            }

            // Now we can construct the actual node.
            let initializer = initializer_runner.drain_into_inner().await;
            let mut joiner_runner =
                Runner::<joiner::Reactor>::new(WithDir::new(root.clone(), initializer), rng)
                    .await?;
            let _ = joiner_runner.run(rng).await;

            let config = joiner_runner
                .drain_into_inner()
                .await
                .into_participating_config()
                .await?;

            network
                .add_node_with_config(config, rng)
                .await
                .expect("could not add node to reactor");
        }

        Ok(network)
    }
}

/// Given an era number, returns a predicate to check if all of the nodes are in the specified era.
fn is_in_era(era_id: EraId) -> impl Fn(&Nodes) -> bool {
    move |nodes: &Nodes| {
        nodes
            .values()
            .all(|runner| runner.participating().consensus().current_era() == era_id)
    }
}

/// A set of consecutive switch blocks.
struct SwitchBlocks {
    headers: Vec<BlockHeader>,
}

impl SwitchBlocks {
    /// Collects all switch blocks of the first `era_count` eras, and asserts that they are equal
    /// in all nodes.
    fn collect(nodes: &Nodes, era_count: u64) -> SwitchBlocks {
        let mut headers = Vec::new();
        for era_number in 0..era_count {
            let mut header_iter = nodes.values().map(|runner| {
                let storage = runner.participating().storage();
                let maybe_block = storage.transactional_get_switch_block_by_era_id(era_number);
                maybe_block.expect("missing switch block").take_header()
            });
            let header = header_iter.next().unwrap();
            assert_eq!(era_number, header.era_id().value());
            for other_header in header_iter {
                assert_eq!(header, other_header);
            }
            headers.push(header);
        }
        SwitchBlocks { headers }
    }

    /// Returns the list of equivocators in the given era.
    fn equivocators(&self, era_number: u64) -> &[PublicKey] {
        &self.headers[era_number as usize]
            .era_end()
            .expect("era end")
            .equivocators
    }

    /// Returns the list of inactive validators in the given era.
    fn inactive_validators(&self, era_number: u64) -> &[PublicKey] {
        &self.headers[era_number as usize]
            .era_end()
            .expect("era end")
            .inactive_validators
    }

    /// Returns the list of validators in the successor era.
    fn next_era_validators(&self, era_number: u64) -> &BTreeMap<PublicKey, U512> {
        self.headers[era_number as usize]
            .next_era_validator_weights()
            .expect("validators")
    }

    /// Returns the set of bids in the auction contract at the end of the given era.
    fn bids(&self, nodes: &Nodes, era_number: u64) -> Bids {
        let correlation_id = Default::default();
        let state_root_hash = *self.headers[era_number as usize].state_root_hash();
        let request = GetBidsRequest::new(state_root_hash.into());
        let runner = nodes.values().next().expect("missing node");
        let engine_state = runner.participating().contract_runtime().engine_state();
        let bids_result = engine_state
            .get_bids(correlation_id, request)
            .expect("get_bids failed");
        bids_result.bids().expect("no bids returned").clone()
    }
}

#[tokio::test]
async fn run_participating_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE);

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // Wait for all nodes to agree on one era.
    net.settle_on(&mut rng, is_in_era(EraId::from(1)), Duration::from_secs(90))
        .await;

    net.settle_on(&mut rng, is_in_era(EraId::from(2)), Duration::from_secs(60))
        .await;
}

#[tokio::test]
async fn run_equivocator_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    let alice_sk = Arc::new(SecretKey::random(&mut rng));
    let alice_pk = PublicKey::from(&*alice_sk);
    let size: usize = 2;
    let mut keys: Vec<Arc<SecretKey>> = (1..size)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();
    let mut stakes: BTreeMap<PublicKey, U512> = keys
        .iter()
        .map(|secret_key| (PublicKey::from(&*secret_key.clone()), U512::from(100)))
        .collect();
    stakes.insert(PublicKey::from(&*alice_sk), U512::from(1));
    keys.push(alice_sk.clone());
    keys.push(alice_sk);

    // We configure the era to take five rounds, and delay all messages to and from one of Alice's
    // nodes until two rounds after genesis. That should guarantee that the two nodes equivocate.
    let mut chain = TestChain::new_with_keys(&mut rng, keys, stakes.clone());
    chain.chainspec_mut().core_config.minimum_era_height = 10;
    let protocol_config = consensus::ProtocolConfig::from(&*chain.chainspec);

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");
    let genesis_time = protocol_config.genesis_timestamp.unwrap();
    let min_round_len = chain.chainspec.highway_config.min_round_length();
    net.reactors_mut()
        .find(|reactor| *reactor.inner().consensus().public_key() == alice_pk)
        .unwrap()
        .set_filter(move |event| match event {
            ParticipatingEvent::NetworkRequest(_)
            | ParticipatingEvent::Consensus(consensus::Event::MessageReceived { .. })
                if Timestamp::now() < genesis_time + min_round_len * 2 =>
            {
                Either::Left(time::sleep(min_round_len.into()).event(move |_| event))
            }
            _ => Either::Right(event),
        });

    let era_count = 3;

    let timeout = Duration::from_secs(90 * era_count);
    info!("Waiting for {} eras to end.", era_count);
    net.settle_on(&mut rng, is_in_era(EraId::new(era_count)), timeout)
        .await;
    let switch_blocks = SwitchBlocks::collect(net.nodes(), era_count);
    let bids: Vec<Bids> = (0..era_count)
        .map(|era_number| switch_blocks.bids(net.nodes(), era_number))
        .collect();

    // In the genesis era, Alice equivocates. Since eviction takes place with a delay of one
    // (`auction_delay`) era, she is still included in the next era's validator set.
    assert_eq!(switch_blocks.equivocators(0), [alice_pk.clone()]);
    assert_eq!(switch_blocks.inactive_validators(0), []);
    assert!(bids[0][&alice_pk].inactive());
    assert!(switch_blocks.next_era_validators(0).contains_key(&alice_pk));

    // In era 1 Alice is banned. Banned validators count neither as faulty nor inactive, even
    // though they cannot participate. In the next era, she will be evicted.
    assert_eq!(switch_blocks.equivocators(1), []);
    assert_eq!(switch_blocks.inactive_validators(1), []);
    assert!(bids[1][&alice_pk].inactive());
    assert!(!switch_blocks.next_era_validators(1).contains_key(&alice_pk));

    // In era 2 she is not a validator anymore and her bid remains deactivated.
    assert_eq!(switch_blocks.equivocators(2), []);
    assert_eq!(switch_blocks.inactive_validators(2), []);
    assert!(bids[2][&alice_pk].inactive());
    assert!(!switch_blocks.next_era_validators(2).contains_key(&alice_pk));

    // We don't slash, so the stakes are never reduced.
    for (pk, stake) in &stakes {
        assert!(bids[0][pk].staked_amount() >= stake);
        assert!(bids[1][pk].staked_amount() >= stake);
        assert!(bids[2][pk].staked_amount() >= stake);
    }

    // The only era with direct evidence is era 0. After that Alice was banned or evicted.
    let none: Vec<&PublicKey> = vec![];
    let alice = vec![&alice_pk];
    for runner in net.nodes().values() {
        let consensus = runner.participating().consensus();
        assert_eq!(consensus.validators_with_evidence(EraId::new(0)), alice);
        assert_eq!(consensus.validators_with_evidence(EraId::new(1)), none);
        assert_eq!(consensus.validators_with_evidence(EraId::new(2)), none);
    }
}
