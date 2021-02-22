#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

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
    do_start_node "5"
    do_start_node "4"
    do_start_node "3"
    # 10-12. Check sync of restarted node
    do_await_full_synchronization "5"
    do_await_full_synchronization "4"
    do_await_full_synchronization "3"
    # 13. Ensure era proceeds after restart
    do_await_era_change "2"
    # 14-16. Check sync of nodes again
    do_await_full_synchronization "5"
    do_await_full_synchronization "4"
    do_await_full_synchronization "3"
    # 17. Verify all nodes are in sync
    check_network_sync
    # 18-20. Compare stalled lfb hash to current
    assert_chain_progressed "5" "$STALLED_LFB"
    assert_chain_progressed "4" "$STALLED_LFB"
    assert_chain_progressed "3" "$STALLED_LFB"

    log "------------------------------------------------------------"
    log "Scenario itst02 complete"
    log "------------------------------------------------------------"
}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    STEP=$((STEP + 1))
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

function do_await_genesis_era_to_complete() {
    log_step "awaiting genesis era to complete"
    while [ "$(get_chain_era)" -lt 1 ]; do
        sleep 1.0
    done
}

function do_read_lfb_hash() {
    local NODE_ID=${1}
    LFB_HASH=$(render_last_finalized_block_hash "$NODE_ID" | cut -f2 -d= | cut -f2 -d ' ')
    echo "$LFB_HASH"
}

function do_stop_node() {
    local NODE_ID=${1}
    log_step "stopping node-$NODE_ID."
    do_node_stop "$NODE_ID"
    sleep 1
}

function do_start_node() {
    local NODE_ID=${1}
    log_step "starting node-$NODE_ID. Syncing from hash=${STALLED_LFB}"
    do_node_start "$NODE_ID" "$STALLED_LFB"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" != "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is not running"
	exit 1
    fi
}

function check_network_sync() {
    local WAIT_TIME_SEC=0
    log_step "check all nodes are in sync"
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if [ "$(do_read_lfb_hash '5')" = "$(do_read_lfb_hash '1')" ] && \
		[ "$(do_read_lfb_hash '4')" = "$(do_read_lfb_hash '1')" ] && \
		[ "$(do_read_lfb_hash '3')" = "$(do_read_lfb_hash '1')" ] && \
		[ "$(do_read_lfb_hash '2')" = "$(do_read_lfb_hash '1')" ]; then
	    log "all nodes in sync, proceeding..."
	    break
        fi

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to confirm network sync"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1
    done
}

function do_await_full_synchronization() {
    local NODE_ID=${1}
    local WAIT_TIME_SEC=0
    log_step "awaiting full synchronization of node=${NODE_ID}…"
    while [ "$(do_read_lfb_hash ${NODE_ID})" != "$(do_read_lfb_hash '1')" ]; do
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to synchronize node-${NODE_ID} in ${SYNC_TIMEOUT_SEC} seconds"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
}

function do_await_era_change() {
    # allow chain height to grow
    local ERA_COUNT=${1:-"1"}
    log_step "awaiting $ERA_COUNT eras…"
    await_n_eras "$ERA_COUNT"
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
