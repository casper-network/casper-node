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
    # 0b. Get era
    STOPPED_ERA=$(check_current_era)
    # 1. Stop node
    do_stop_node "5"
    # 2. Let the node go down
    log_step "Sleeping for 5s"
    sleep 5
    # 3. Restart Node
    do_read_lfb_hash 1
    do_start_node "5" "$LFB_HASH"
    # 4. Verify all nodes are in sync
    check_network_sync
    # 5. Verify network chain height sync
    assert_chainheight_network_sync
    # 6. Send deploy to stopped node
    send_single_native_transfer '5'
    # 7. Verify deploy is finalized
    assert_finalized_deploy "$DEPLOY_HASH"
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

function send_single_native_transfer() {
    unset DEPLOY_HASH
    local NODE_ID=${1}
    log_step "sending 1 native transfer to $NODE_ID"
    local TRANSFER_OUT=$(nctl-transfer-native node="$NODE_ID" ammount=2500000000 transfers=1 user=1 | grep '#1')
    DEPLOY_HASH=$(echo $TRANSFER_OUT | awk -F':: ' '{print $4}')
    echo $DEPLOY_HASH
}

function assert_finalized_deploy() {
    local DEPLOY_HASH=${1}
    local WAIT_TIME_SEC=0
    log_step "Checking all validators for finalized deploy: $DEPLOY_HASH"
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        local NETWORK_FINALIZED=$(cat "$NCTL"/assets/net-1/nodes/node-*/logs/stdout.log | grep 'finalized_block' | grep "DeployHash($DEPLOY_HASH)" | wc -l)
        if [ "$NETWORK_FINALIZED" = '5' ]; then
            log "All validators finalized deploy: $DEPLOY_HASH"
            break
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to confirm network sync"
            exit 1
        fi
        sleep 1
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

# Normally I'd just wait an era and re-check with check_network_sync
# but we need to remain in the era. This will verify we caught up
# prior to firing deploys.
function assert_chainheight_network_sync() {
    log_step "Checking network chain height in sync..."
    local WAIT_TIME_SEC=0
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if [ "$(get_chain_height '5')" = "$(get_chain_height '1')" ] && \
                [ "$(get_chain_height '4')" = "$(get_chain_height '1')" ] && \
                [ "$(get_chain_height '3')" = "$(get_chain_height '1')" ] && \
                [ "$(get_chain_height '2')" = "$(get_chain_height '1')" ]; then
            log "Network chain height in sync, proceeding..."
            break
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to confirm chain height in sync"
            exit 1
        fi
        sleep 1
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset DEPLOY_HASH
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
