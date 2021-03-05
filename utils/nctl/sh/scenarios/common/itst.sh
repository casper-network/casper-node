#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

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
    local NODE_ID=${1}
    LFB_HASH=$(render_last_finalized_block_hash "$NODE_ID" | cut -f2 -d= | cut -f2 -d ' ')
    echo "$LFB_HASH"
}

function do_start_node() {
    local NODE_ID=${1}
    local LFB_HASH=${2}
    log_step "starting node-$NODE_ID. Syncing from hash=${LFB_HASH}"
    do_node_start "$NODE_ID" "$LFB_HASH"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" != "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is not running"
	exit 1
    else
        log "node-${NODE_ID} is running"
    fi
}

function do_stop_node() {
    local NODE_ID=${1}
    log_step "stopping node-$NODE_ID."
    do_node_stop "$NODE_ID"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" = "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is still running"
	exit 1
    else
        log "node-${NODE_ID} was shutdown in era: $(check_current_era)"
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
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to confirm network sync"
            exit 1
        fi
        sleep 1
    done
}

function do_await_era_change() {
    # allow chain height to grow
    local ERA_COUNT=${1:-"1"}
    log_step "awaiting $ERA_COUNT eras…"
    await_n_eras "$ERA_COUNT"
}

function check_current_era {
    echo $(nctl-view-chain-era node=$(get_node_for_dispatch) | awk '{print $NF}')
}
