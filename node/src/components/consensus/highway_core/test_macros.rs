//! Macros for concise test setup.

/// Creates a panorama from a list of either observations or unit hashes. Unit hashes are converted
/// to `Correct` observations.
macro_rules! panorama {
    ($($obs:expr),*) => {{
        use crate::components::consensus::highway_core::state::Panorama;

        Panorama::from(vec![$($obs.into()),*])
    }};
}

/// Creates a unit, adds it to `$state` and returns its hash.
/// Returns an error if unit addition fails.
///
/// The short variant is for tests that don't care about timestamps and round lengths: It
/// automatically picks reasonable values for those.
macro_rules! add_unit {
    ($state: ident, $creator: expr, $val: expr; $($obs:expr),*) => {{
        add_unit!($state, $creator, $val; $($obs),*;)
    }};
    ($state: ident, $creator: expr, $val: expr; $($obs:expr),*; $($ends:expr),*) => {{
        #[allow(unused_imports)] // These might be already imported at the call site.
        use crate::{
            components::consensus::highway_core::{
                state::{self, tests::TestSecret},
                highway::{SignedWireUnit, WireUnit},
                highway_testing::TEST_INSTANCE_ID,
            },
        };

        #[allow(unused_imports)] // These might be already imported at the call site.
        use casper_types::{TimeDiff, Timestamp};

        let creator = $creator;
        let panorama = panorama!($($obs),*);
        let seq_number = panorama.next_seq_num(&$state, creator);
        let maybe_parent_hash = panorama[creator].correct();
        // Use our most recent round length, or the configured initial one.
        let r_len = maybe_parent_hash.map_or_else(
            || $state.params().init_round_len(),
            |vh| $state.unit(vh).round_len(),
        );
        let value = Option::from($val);
        // At most two units per round are allowed.
        let two_units_limit = maybe_parent_hash
            .and_then(|ph| $state.unit(ph).previous())
            .map(|pph| $state.unit(pph))
            .map(|unit| unit.round_id() + unit.round_len());
        // And our timestamp must not be less than any justification's.
        let mut timestamp = panorama
            .iter_correct(&$state)
            .map(|unit| unit.timestamp + TimeDiff::from(1))
            .chain(two_units_limit)
            .max()
            .unwrap_or($state.params().start_timestamp());
        // If this is a block: Find the next time we're a leader.
        if value.is_some() {
            timestamp = state::round_id(timestamp + r_len - TimeDiff::from(1), r_len);
            while $state.leader(timestamp) != creator {
                timestamp += r_len;
            }
        }
        let round_exp = (r_len / $state.params().min_round_length()).trailing_zeros() as u8;
        let wunit = WireUnit {
            panorama,
            creator,
            instance_id: TEST_INSTANCE_ID,
            value,
            seq_number,
            timestamp,
            round_exp,
            endorsed: vec![$($ends),*].into_iter().collect(),
        };
        let hwunit = wunit.into_hashed();
        let hash = hwunit.hash();
        let swunit = SignedWireUnit::new(hwunit, &TestSecret(($creator).0));
        $state.add_unit(swunit).map(|()| hash)
    }};
    ($state: ident, $creator: expr, $time: expr, $round_exp: expr, $val: expr; $($obs:expr),*) => {{
        add_unit!($state, $creator, $time, $round_exp, $val; $($obs),*; std::collections::BTreeSet::new())
    }};
    ($state: ident, $creator: expr, $time: expr, $round_exp: expr, $val: expr; $($obs:expr),*; $($ends:expr),*) => {{
        use crate::components::consensus::highway_core::{
            state::tests::TestSecret,
            highway::{SignedWireUnit, WireUnit},
            highway_testing::TEST_INSTANCE_ID,
        };

        let creator = $creator;
        let panorama = panorama!($($obs),*);
        let seq_number = panorama.next_seq_num(&$state, creator);
        let wunit = WireUnit {
            panorama,
            creator,
            instance_id: TEST_INSTANCE_ID,
            value: ($val).into(),
            seq_number,
            timestamp: ($time).into(),
            round_exp: $round_exp,
            endorsed: $($ends.into()),*
        };
        let hwunit = wunit.into_hashed();
        let hash = hwunit.hash();
        let swunit = SignedWireUnit::new(hwunit, &TestSecret(($creator).0));
        $state.add_unit(swunit).map(|()| hash)
    }};
}

/// Creates an endorsement of `vote` by `creator` and adds it to the state.
macro_rules! endorse {
    ($state: ident, $vote: expr; $($creators: expr),*) => {
        let creators = vec![$($creators.into()),*];
        for creator in creators.into_iter() {
            endorse!($state, creator, $vote);
        }
    };
    ($state: ident, $creator: expr, $vote: expr) => {{
        use crate::components::consensus::highway_core::endorsement::{
            Endorsement, SignedEndorsement,
        };

        let endorsement: Endorsement<TestContext> = Endorsement::new($vote, ($creator));
        let signature = TestSecret(($creator).0).sign(&endorsement.hash());
        let endorsements = SignedEndorsement::new(endorsement, signature).into();
        let evidence = $state.find_conflicting_endorsements(&endorsements, &TEST_INSTANCE_ID);
        $state.add_endorsements(endorsements);
        for ev in evidence {
            $state.add_evidence(ev);
        }
    }};
}
