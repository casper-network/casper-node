#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration test that tries to simulate
# and verify validator ejection and rejoining.
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst13"
    log "------------------------------------------------------------"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Allow chain to progress
    do_await_era_change
    # 2. Verify all nodes are in sync
    check_network_sync
    # 3. Stop the node
    do_stop_node '5'
    # 4. Wait until N+1
    do_await_era_change '1'
    await_n_blocks '1' 'true'
    get_switch_block '1' '100'
    # Wait 1 extra block to avoid potential overlap.
    await_n_blocks '1' 'true'
    # Gather Block Hash after stopping node for walkback later
    local RESTART_HASH=$(do_read_lfb_hash '1')
    # 5. Wait until N+2
    do_await_era_change '1'
    await_n_blocks '1' 'true'
    get_switch_block '1' '100'
    # 6. Assert node is marked as inactive
    assert_inactive '5'

    # 7. Re-bid & restart node 5
    # CASE 1: Comment out and uncomment CASE 2 for comparison
    do_start_node '5' "$(get_chain_first_block_hash)"
    assert_joined_in_era_4 '5'

    # 8. Assert eviction of node
    do_await_era_change '1'
    await_n_blocks '1' 'true'
    get_switch_block '1' '100'
    assert_eviction '5'
    # 9. Assert node didn't propose since being shutdown
    assert_no_proposal_walkback '5' "$RESTART_HASH"
    # 10. Re-bid shutdown node
    do_submit_auction_bids '5'

    # CASE 2: Comment out and uncomment CASE 1 for comparison
    #do_start_node '5' "$RESTART_HASH"

    # 11. wait auction_delay + 1 + 1 more for partial era protection
    # NOTE: auction_delay = 1 for this test.
    do_await_era_change '3'
    # 12. Assert that restarted validator is producing blocks.
    assert_node_proposed '5' '300'
    # 13. Check for equivocators
    assert_no_equivocators_logs
    log "------------------------------------------------------------"
    log "Scenario itst13 complete"
    log "------------------------------------------------------------"
}

function assert_joined_in_era_4() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node "$NODE_ID")
    local TIMEOUT=${2:-300}
    log_step "Waiting for a node-$NODE_ID to join..."
    local OUTPUT=$(timeout "$TIMEOUT" tail -n 1 -f "$NODE_PATH/logs/stdout.log" | grep -o -m 1 "finished joining")
    if ( echo "$OUTPUT" | grep -q "finished joining" ); then
        log "Node-$NODE_ID joined!"
        log "$OUTPUT"
    else
        log "ERROR: Node-$NODE_ID didn't join within timeout=$TIMEOUT"
        exit 1
    fi

    assert_same_era '4' '1'
}

# Checks that a validator gets marked as inactive
function check_inactive() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex_extended "$NODE_ID")
    # In order to pass bash variables into jq you must specify a jq arg.
    # Below the jq arg 'node_hex' is set to $HEX. The query looks at the
    # state of the auction and checks to see if a validator gets marked
    # as inactive. The validator is found via his public key $HEX (node_hex).
    # We return the exit code of the grep to check success.
    nctl-view-chain-auction-info | jq --arg node_hex "$HEX" '.auction_state.bids[] | select(.public_key == $node_hex).bid.inactive' | grep -q 'true'
    return $?
}

# Checks that the current era + 1 contains a nodes 
# public key hex
function is_trusted_validator() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex_extended "$NODE_ID")
    local ERA=$(check_current_era)
    # Plus 1 to avoid query issue if era switches mid run
    local ERA_PLUS_1=$(expr $ERA + 1)
    # note: tonumber is a must here to prevent jq from being too smart.
    # The jq arg 'era' is set to $ERA_PLUS_1. The query looks to find that
    # the validator is removed from era_validators list. We grep for
    # the public_key_hex to see if the validator is still listed and return
    # the exit code to check success.
    nctl-view-chain-auction-info | jq --arg era "$ERA_PLUS_1" '.auction_state.era_validators[] | select(.era_id == ($era | tonumber))' | grep -q "$HEX"
    return $?
}

function assert_inactive() {
    local NODE_ID=${1}
    log_step "Checking for inactive node-$NODE_ID..."
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if ( check_inactive "$NODE_ID" ); then
            log "validator node-$NODE_ID is inactive! [expected]"
            break
        fi

        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Time out. Failed to confirm node-$NODE_ID as inactive validator in $SYNC_TIMEOUT_SEC seconds."
            exit 1
        fi
        sleep 1
    done
}

function assert_eviction() {
    local NODE_ID=${1}
    log_step "Checking for evicted node-$NODE_ID..."
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if ( ! is_trusted_validator "$NODE_ID" ); then # && ( check_inactive "$NODE_ID" ); then
            log "validator node-$NODE_ID was ejected! [expected]"
            break
        fi

        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Time out. Failed to confirm node-$NODE_ID as evicted validator in $SYNC_TIMEOUT_SEC seconds."
            exit 1
        fi
        sleep 1
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
STEP=0

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        timeout) SYNC_TIMEOUT_SEC=${VALUE} ;;
        *) ;;
    esac
done

SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}

main
