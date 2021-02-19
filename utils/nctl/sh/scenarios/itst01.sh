#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/utils/infra.sh

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

    log "Node to be stopped: $NODE_ID"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Allow chain to progress
    do_await_era_change
    # 2. Stop random node
    do_stop_node "$NODE_ID"
    # 3. Allow chain to progress
    do_await_era_change
    # 4. Get another random running node to compare
    do_get_another_node
    do_read_lfb_hash "$COMPARE_NODE_ID"
    # 5. Restart node from LFB
    do_start_node
    # 6-8. Check sync of restarted node
    do_await_full_synchronization

    log "------------------------------------------------------------"
    log "Scenario itst01 complete"
    log "------------------------------------------------------------"
}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    STEP=$((STEP + 1))
}

function do_await_genesis_era_to_complete() {
    log_step "awaiting genesis era to complete"
    while [ "$(get_chain_era)" -lt 1 ]; do
        sleep 1.0
    done
}

function do_read_lfb_hash() {
    LFB_HASH=$(get_chain_latest_block_hash)
    echo "$LFB_HASH"
}

function do_read_lfb_hash() {
    local NODE_ID=${1}
    LFB_HASH=$(render_last_finalized_block_hash "$NODE_ID" | cut -f2 -d= | cut -f2 -d ' ')
    echo "$LFB_HASH"
}

function do_stop_node() {
    log_step "stopping node-$NODE_ID."
    do_node_stop "$NODE_ID"
}

function do_start_node() {
    log_step "starting node-$NODE_ID. Syncing from hash=${LFB_HASH}"
    do_node_start "$NODE_ID" "$LFB_HASH"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" != "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is not running"
	exit 1
    fi
}

function do_await_full_synchronization() {
    local WAIT_TIME_SEC=0
    log_step "awaiting full synchronization of node=${NODE_ID}…"
    while [ "$(do_read_lfb_hash "$NODE_ID")" != "$(do_read_lfb_hash "$COMPARE_NODE_ID")" ]; do
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to synchronize in ${SYNC_TIMEOUT_SEC} seconds"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
    # Wait 1 era and then check the LFB.
    # This way we can verify that the node is up-to-date with the protocol state
    # after transitioning to an active validator.
    do_await_era_change
    log_step "verifying full synchronization of node=${NODE_ID}…"
    while [ "$(do_read_lfb_hash "$NODE_ID")" != "$(do_read_lfb_hash "$COMPARE_NODE_ID")" ]; do
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to keep up with the protocol state"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
}

function do_await_era_change() {
    # allow chain height to grow
    log_step "awaiting 1 eras…"
    await_n_eras 1
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

NODE_ID=$(get_node_for_dispatch)
SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}

main "$NODE_ID"
