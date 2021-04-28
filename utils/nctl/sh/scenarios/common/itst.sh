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
    while [ "$(get_chain_era)" != "1" ]; do
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
    log_step "check all node's LFBs are in sync"
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
    local ERA="null"
    while true; do
        ERA=$(get_chain_era $(get_node_for_dispatch) | awk '{print $NF}')
        if [ "$ERA" = "null" ] || [ "$ERA" = "N/A" ]; then
            sleep 1
        else
            break
        fi
    done
    echo "$ERA"
}

function check_faulty() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    grep -q 'this validator is faulty' "$NODE_PATH/logs/stdout.log"
    return $?
}

# Captures the public key in hex of a node minus the '01' prefix.
# This is because the logs don't include the '01' prefix when
# searching in them for the public key in hex. See itst14 for
# use case.
function get_node_public_key_hex() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(cat "$NODE_PATH"/keys/public_key_hex)
    echo "${PUBLIC_KEY_HEX:2}"
}

# Same as get_node_public_key_hex but includes the '01' prefix.
# When parsing the state with jq, the '01' prefix is included.
# See itst13 for use case.
function get_node_public_key_hex_extended() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(cat "$NODE_PATH"/keys/public_key_hex)
    echo "$PUBLIC_KEY_HEX"
}

function do_await_n_blocks() {
    local BLOCK_COUNT=${1:-1}
    log_step "Waiting $BLOCK_COUNT blocks..."
    nctl-await-n-blocks offset="$BLOCK_COUNT"
}

function get_switch_block_equivocators() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local BLOCK_HASH=${3}
    local JSON_OUT
    local PARENT
    local BLOCK_HEADER

    if [ -z "$BLOCK_HASH" ]; then
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID"))
    else
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID") -b "$BLOCK_HASH")
    fi

    if [ "$WALKBACK" -gt 0 ]; then
        BLOCK_HEADER=$(echo "$JSON_OUT" | jq '.result.block.header')
        if [ "$(echo "$BLOCK_HEADER" | jq '.era_end')" = "null" ]; then
            PARENT=$(echo "$BLOCK_HEADER" | jq -r '.parent_hash')
            WALKBACK=$((WALKBACK - 1))
            log "$WALKBACK: Walking back to block: $PARENT"
            get_switch_block_equivocators "$NODE_ID" "$WALKBACK" "$PARENT"
        else
            log "equivocators: $(echo "$BLOCK_HEADER" | jq '.era_end.era_report.equivocators')"
        fi
    else
        log "Error: Switch block not found within walkback!"
        exit 1
    fi
}

# Function is used to walk back the blocks to check if a transfer
# is included under transfer_hashes. If the transfer is not found
# within the walkback, it will error out.
function verify_transfer_inclusion() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local TRANSFER=${3}
    local BLOCK_HASH=${4}
    local JSON_OUT
    local PARENT
    local BLOCK_HEADER
    local BLOCK_TRANSFER_HASHES

    if [ -z "$BLOCK_HASH" ]; then
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID"))
    else
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID") -b "$BLOCK_HASH")
    fi

    if [ "$WALKBACK" -gt 0 ]; then
        BLOCK_HEADER=$(echo "$JSON_OUT" | jq '.result.block.header')
        BLOCK_TRANSFER_HASHES=$(echo "$JSON_OUT" | jq -r '.result.block.body.transfer_hashes[]')
        if grep -q "${TRANSFER}" <<< "$BLOCK_TRANSFER_HASHES"; then
            log "Transfer: $TRANSFER found in block!"
        else
            PARENT=$(echo "$BLOCK_HEADER" | jq -r '.parent_hash')
            WALKBACK=$((WALKBACK - 1))
            log "$WALKBACK: Walking back to block: $PARENT"
            verify_transfer_inclusion "$NODE_ID" "$WALKBACK" "$TRANSFER" "$PARENT"
        fi
    else
        log "Error: Transfer $TRANSFER not found within walkback!"
        exit 1
    fi
}

function get_running_node_count {
    local RUNNING_COUNT=$(nctl-status | grep 'RUNNING' | wc -l)
    echo "$RUNNING_COUNT"
}

# Check that a certain node has produced blocks.
function assert_node_proposed() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(get_node_public_key_hex $NODE_ID)
    log_step "Waiting for node-$NODE_ID to produce a block..."
    local OUTPUT=$(tail -f "$NODE_PATH/logs/stdout.log" | grep -m 1 "proposer: PublicKey::Ed25519($PUBLIC_KEY_HEX)")
    log "node-$NODE_ID created a block!"
    log "$OUTPUT"
}

# Checks logs for nodes 1-10 for equivocators
function assert_no_equivocators_logs() {
    local LOGS
    log_step "Looking for equivocators in logs..."
    for i in $(seq 1 $(get_count_of_nodes)); do
        # true is because grep exits 1 on no match
        LOGS=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w 'validator equivocated') || true
        if [ ! -z "$LOGS" ]; then
            log "$(echo $LOGS)"
            log "Fail: Equivocator(s) found!"
            exit 1
        fi
    done
}
