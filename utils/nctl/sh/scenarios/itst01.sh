#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that tries to simulate
# stopping and restarting a random node.
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst01"
    log "------------------------------------------------------------"

    local NODE_SHUTDOWN_ID

    # 0. Wait for network start up
    do_await_genesis_era_to_complete

    NODE_SHUTDOWN_ID=$(get_node_for_dispatch)
    log "Node to be stopped: $NODE_SHUTDOWN_ID"

    # 1. Allow chain to progress
    do_await_era_change
    # 2. Stop random node
    do_stop_node "$NODE_SHUTDOWN_ID"
    # 3. Allow chain to progress
    do_await_era_change
    # 4. Get another random running node to compare
    do_get_another_node
    do_read_lfb_hash "$COMPARE_NODE_ID"
    # 5. Restart node from LFB
    do_start_node "$NODE_SHUTDOWN_ID" "$LFB_HASH"
    # 6-8. Check sync of restarted node,
    # wait 1 era, and then check they are still in sync.
    # This way we can verify that the node is up-to-date with the protocol state
    # after transitioning to an active validator.
    parallel_check_network_sync 1 5 
    do_await_era_change
    parallel_check_network_sync 1 5
    # 9. Run Closing Health Checks
    # ... restarts=1: due to node being stopped and started
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=1 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario itst01 complete"
    log "------------------------------------------------------------"
}

function do_get_another_node() {
    COMPARE_NODE_ID=$(get_node_for_dispatch)
    log_step "comparison node: $COMPARE_NODE_ID"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
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
