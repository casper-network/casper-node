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

    # 0a. Verify all nodes are in sync
    check_network_sync
    log "Sleeping 10s"
    sleep 10
    check_network_sync
    # 0b. Get era
    STOPPED_ERA=$(check_current_era)
    # 1. Stop node
    do_stop_node "5"
    # 2. Let the node go down
    log_step "Sleeping for 10s"
    sleep 10
    # 3. Restart Node
    do_read_lfb_hash 1
    do_start_node "5" "$LFB_HASH"
    # 4. Verify all nodes are in sync
    check_network_sync
    # 5. Verify network chain height sync
    log "Sleeping 10s"
    sleep 10
    check_network_sync
    # 6. Send deploy to stopped node
    send_transfers '100'
    # 7. Verify network finalized deploy
    assert_node_proposed '5'
    # 8. Check we are in the same era
    assert_same_era "$STOPPED_ERA"
    # 9. Wait an era
    do_await_era_change
    # 10. Verify all nodes are in sync
    check_network_sync
    # 11. Check for equivication
    check_node_equivocated '5'

    log "------------------------------------------------------------"
    log "Scenario itst14 complete"
    log "------------------------------------------------------------"
}

function assert_same_era() {
    local ERA=${1}
    log_step "Checking if within same era..."
    if [ "$ERA" == "$(check_current_era)" ]; then
        log "Still within the era. Continuing..."
    else
        log "Error: Era progressed! Exiting..."
        exit 1
    fi
}

function assert_node_proposed() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(get_node_public_key_hex $NODE_ID)
    log_step "Waiting for a node-$NODE_ID to produce a block..."
    local OUTPUT=$(tail -f "$NODE_PATH/logs/stdout.log" | grep -m 1 "proposer: PublicKey::Ed25519($PUBLIC_KEY_HEX)")
    log "node-$NODE_ID created a block!"
    log "$OUTPUT"
}

function send_transfers() {
    local NUM_T=${1:-1}
    local NUM_RUNNING_NODES=$(nctl-status | grep -i 'running' | wc -l)
    log_step "sending $NUM_T native transfers to $NUM_RUNNING_NODES running nodes..."
    for ((i=0; i<=$NUM_RUNNING_NODES; i++)); do
        log "node-$i: $NUM_T transfers sent."
        local TRANSFER_OUT=$(nctl-transfer-native node=$i ammount=2500000000 transfers="$NUM_T" user=1)
    done
}

function check_node_equivocated() {
    local NODE_ID=${1}
    log_step "Checking to see if node-$NODE_ID equivocated..."
    if ( check_faulty "$NODE_ID" ); then
        log "Error: Node-$NODE_ID equivocated!"
        exit 1
    else
        log "Node-$NODE_ID did not equivocate! yay!"
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
