#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration test that tries to simulate
# and verify validator ejection.
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
    # 4. wait auction_delay + 1
    do_await_era_change '4'
    # 5. Validate eviction occured
    assert_eviction '5'
    # 6. Check for equivocators
    assert_no_equivocators_logs

    log "------------------------------------------------------------"
    log "Scenario itst13 complete"
    log "------------------------------------------------------------"
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

function assert_eviction() {
    local NODE_ID=${1}
    log_step "Checking for evicted node-$NODE_ID..."
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if ( ! is_trusted_validator "$NODE_ID" ) && ( check_inactive "$NODE_ID" ); then
            log "validator node-$NODE_ID was ejected! [expected]"
            break
        fi

        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Time out. Failed to confirm a faulty validator in $SYNC_TIMEOUT_SEC seconds."
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
