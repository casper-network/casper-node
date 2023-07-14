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
    local NODE_STARTUP_ERA

    log "------------------------------------------------------------"
    log "Starting Scenario: itst13"
    log "------------------------------------------------------------"

    # Wait for network start up
    do_await_genesis_era_to_complete
    # Verify all nodes are in sync
    parallel_check_network_sync 1 5
    # Stop the node
    do_stop_node '5'
    # Wait until N+1
    do_await_era_change '1'
    # Wait 1 block to avoid missing latest switch block
    await_n_blocks '1' 'true'
    # Get concluded era's switch block
    get_switch_block '1' '100'
    # Gather Block Hash after stopping node for walkback later
    local RESTART_HASH=$(do_read_lfb_hash '1')
    # Wait until N+2
    do_await_era_change '1'
    # Wait 1 block to avoid missing latest switch block
    await_n_blocks '1' 'true'
    # Get concluded era's switch block
    get_switch_block '1' '100'
    # Assert node is marked as inactive
    assert_inactive '5'
    # Remember current era
    NODE_STARTUP_ERA=$(check_current_era)
    log "I'll expect the node to switch to KeepUp within era=$NODE_STARTUP_ERA..."
    # Restart node 5
    do_start_node '5' "$(do_read_lfb_hash '1')"
    # Assert joined within expected era
    assert_joined_in_era_x '5' '300' $NODE_STARTUP_ERA
    # Assert eviction of node
    do_await_era_change '1'
    # Wait 1 block to avoid missing latest switch block
    await_n_blocks '1' 'true'
    # Get concluded era's switch block
    get_switch_block '1' '100'
    # Assert node 5 was evicted
    assert_eviction '5'
    # Node 5 must be synced to genesis in case it's selected for walkback
    await_node_historical_sync_to_genesis '5' '300'
    # Assert node didn't propose since being shutdown
    assert_no_proposal_walkback '5' "$RESTART_HASH"
    # Re-bid shutdown node
    do_submit_auction_bids '5'
    # wait auction_delay + 1 + 1 more for partial era protection
    # NOTE: auction_delay = 1 for this test.
    do_await_era_change '3'
    # Assert that restarted validator is producing blocks.
    assert_node_proposed '5' '300'
    # Run Health Checks
    # ... restarts=1: due to node being stopped and started
    # ... ejections=1: node is expected to be ejected in test
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=1 \
            ejections=1

    log "------------------------------------------------------------"
    log "Scenario itst13 complete"
    log "------------------------------------------------------------"
}

function assert_joined_in_era_x() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node "$NODE_ID")
    local TIMEOUT=${2:-300}
    local NODE_STARTUP_ERA=${3}
    local OUTPUT
    local REACTOR_STATE

    log_step "Waiting for node-$NODE_ID to join..."
    while true; do
        OUTPUT=$(nctl-view-node-status node="$NODE_ID")
        REACTOR_STATE=$(echo "$OUTPUT" | tail -n +2 | jq -r '.reactor_state')

        if [ "$REACTOR_STATE" == "KeepUp" ] || [ "$REACTOR_STATE" == "Validate" ]; then
            log "Node-$NODE_ID joined!"
            log "Node-$NODE_ID's reactor_state = $REACTOR_STATE"
            break
        else
            sleep 1
            TIMEOUT=$((TIMEOUT-1))
            if [ "$TIMEOUT" = '0' ]; then
                log "ERROR: Timed out before joining"
                exit 1
            else
                log "...waiting for node to be in sync: timeout=$TIMEOUT"
                log "...node-$NODE_ID's reactor_state = $REACTOR_STATE"
            fi
        fi
    done

    assert_same_era $NODE_STARTUP_ERA '1'
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
