#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that tries to simulate
# stalling consensus by stopping enough nodes. It
# then restarts the nodes and checks for the chain
# to progress.
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst02"
    log "------------------------------------------------------------"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Allow chain to progress
    do_await_era_change
    # 2. Verify all nodes are in sync
    check_network_sync
    # 3-5. Stop three nodes
    do_stop_node "5"
    do_stop_node "4"
    do_stop_node "3"
    # 6. Ensure chain stalled
    assert_chain_stalled "60"
    # 7-9. Restart three nodes
    do_start_node "5" "$STALLED_LFB"
    do_start_node "4" "$STALLED_LFB"
    do_start_node "3" "$STALLED_LFB"
    # 10. Verify all nodes are in sync
    check_network_sync
    # 11. Ensure era proceeds after restart
    do_await_era_change "2"
    # 12. Verify all nodes are in sync
    check_network_sync
    # 13-15. Compare stalled lfb hash to current
    assert_chain_progressed "5" "$STALLED_LFB"
    assert_chain_progressed "4" "$STALLED_LFB"
    assert_chain_progressed "3" "$STALLED_LFB"

    log "------------------------------------------------------------"
    log "Scenario itst02 complete"
    log "------------------------------------------------------------"
}

function assert_chain_progressed() {
    # Function accepts two hashes as arguments and checks to
    # see if they match.
    log_step "node-${1}: checking chain progressed"
    local LFB1=$(do_read_lfb_hash ${1})
    local LFB2=${2}

    if [ "$LFB1" = "$LFB2" ]; then
       log "error: $LFB1 = $LFB2, chain didn't progress."
       exit 1
    fi
}

function assert_chain_stalled() {
    # Fucntion checks that the two remaining node's LFB checked
    # n-seconds apart doesnt progress
    log_step "ensuring chain stalled"
    local SLEEP_TIME=${1}
    # Sleep 5 seconds to allow for final message propagation.
    sleep 5
    local LFB_1_PRE=$(do_read_lfb_hash 1)
    local LFB_2_PRE=$(do_read_lfb_hash 2)
    log "Sleeping ${SLEEP_TIME}s..."
    sleep $SLEEP_TIME
    local LFB_1_POST=$(do_read_lfb_hash 1)
    local LFB_2_POST=$(do_read_lfb_hash 2)

    if [ "$LFB_1_PRE" != "$LFB_1_POST" ] && [ "$LFB_2_PRE" != "$LFB_2_POST" ]; then
       log "Error: Chain progressed."
       exit 1
    fi

    if [ "$LFB_1_POST" != "$LFB_2_POST" ]; then
        log "Error: LFB mismatch on nodes"
	exit 1
    else
        STALLED_LFB=$LFB_1_POST
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset STALLED_LFB
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

main "$NODE_ID"
