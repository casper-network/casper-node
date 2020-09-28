use casper_engine_test_support::internal::{
    InMemoryWasmTestBuilder, StepItem, StepRequestBuilder, DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_types::{bytesrepr::ToBytes, ProtocolVersion, U512};

// use casper_execution_engine::core::engine_state::step::StepRequest;

/*
TODO's:
* add new endpoint, request, and response to ipc.proto engine state service
** what should request / response contain?
*** set up similar to exec / deploy w/ oneof's
*** add Slashing first & wire all the way thru, should be easy to add more variants thereafter
* add new step function, domain request & response types to EE engine_state
* add From mappings for ipc / domain types in grpc host
* implement call to auction slashing in EE function; self commit afterwards.
* iterate until should_slash test passes

* Implement should_run_auction test; iterate to success
* Implement should_distribute_rewards test; iterate to success
* create a separate EE ticket to track this portion of the work and submit a PR.

* Complete wire up of ContractRuntime & BlockExecutor on ndrs-307 story and cut a
    second PR based on the first.
*/

/// Should be able to apply slashing per era.
#[ignore]
#[test]
fn should_slash() {
    /*
        get a builder & do genesis plus some number of commits to build up state
            how much min state is necessary before being able to slash?
                maybe genesis + 1 block / post state hash is all that is necessary; verify
            should be able to just assert era 2 as to the EE era is just an arg
        slash a validator
            add a .step(...) function to the test builder
            check the auction contract validator set before slash and pick a victim
        how to determine success?
            check auction contract state after slash; victim should be gone and all others
            should still exist.
    */
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
    let validator_id = casper_types::PublicKey::Ed25519([42; 32])
        .to_bytes()
        .expect("should serialize to bytes");
    let step_request = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_slash_item(StepItem::new(validator_id, U512::from(1000)).as_slash_item())
        .build();
    builder.step(step_request);
}

/// Should be able to apply slashing per era.
#[ignore]
#[test]
fn should_run_auction() {
    /*
        get a builder & do genesis plus some number of commits to build up state
            how much min state is necessary before being able to auction?
                maybe genesis + 1 block / post state hash is all that is necessary; verify
            should be able to just assert era 2 as to the EE era is just an arg
        run auction in such a way to force a new winning bidder / new validator to be included
            upgrade .step(...) function on test builder to support auction
            check the auction contract validator set before auction
        how to determine success?
            check auction contract state after auction; new winning bidder should be present

        TODO: check Michal's existing tests of run_auction to figure out how it is meant to work
    */
}

/// Should be able to distribute rewards per era.
#[ignore]
#[test]
fn should_distribute_rewards() {
    /*
        get a builder & do genesis plus some number of commits to build up state
            how much min state is necessary before being able to distro rewards?
                maybe genesis + 1 block / post state hash is all that is necessary; verify
            should be able to just assert era 2 as to the EE era is just an arg
        distribute rewards a validator
            upgrade .step(...) function on test builder to support reward distro
            determine what state is available pre-reward distro to serve as control set
        how to determine success?
            determine what state is available post-reward distro to compare against previous state
                and what the expected final resting state is; may not be able to do this
                until henry's new logic completely lands in which case, allow test to pass
                but add TODO! and link to Henry's ticket.
    */
}
