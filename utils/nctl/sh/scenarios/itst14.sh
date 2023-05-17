#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that tries to simulate
# if a validator node can restart within single era
# and not equivocate.
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst14"
    log "------------------------------------------------------------"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Verify network is creating blocks
    do_await_n_blocks "5"
    # 2. Verify network is in sync
    parallel_check_network_sync 1 5
    # 3a. Get era
    STOPPED_ERA=$(check_current_era)
    # 3b. Stop node
    do_stop_node "5"
    # 4. Let the node go down
    log_step "Sleeping for 10s before bringing node back online..."
    sleep 10
    # 5. Restart Node
    do_read_lfb_hash 1
    do_start_node "5" "$LFB_HASH"
    # 6. Verify all nodes are in sync
    parallel_check_network_sync 1 5
    # 7. Verify network is creating blocks post-restart
    do_await_n_blocks "5"
    # 8. Verify all nodes are in sync
    parallel_check_network_sync 1 5
    # 9. Verify node proposed a block
    assert_node_proposed '5' '180'
    # 10. Verify we are in the same era
    assert_same_era "$STOPPED_ERA"
    # 11. Wait an era
    do_await_era_change
    # 12. Verify all nodes are in sync
    parallel_check_network_sync 1 5
    # 13. Run Health Checks
    # ... restarts=1: due to node being stopped and started
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=1 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario itst14 complete"
    log "------------------------------------------------------------"
}

function assert_no_equivocation() {
    local NODE_ID=${1}
    local QUERY_NODE_ID=${2}
    local WALKBACK=${3}
    local EQUIVOCATORS
    # "casper-client list-rpc" shows this including '01' prefix. Using extended version.
    local PUBLIC_KEY_HEX=$(get_node_public_key_hex_extended "$NODE_ID")
    log_step "Checking to see if node-$NODE_ID:$PUBLIC_KEY_HEX equivocated..."
    EQUIVOCATORS=$(get_switch_block_equivocators "$QUERY_NODE_ID" "$WALKBACK")
    log "$EQUIVOCATORS"
    if ( ! echo "$EQUIVOCATORS" | grep -q "$PUBLIC_KEY_HEX" ); then
        log "Node-$NODE_ID didn't equivocate! yay!"
    else
        log "ERROR: Node-$NODE_ID equivocated!"
        exit 1
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset PUBLIC_KEY_HEX
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
