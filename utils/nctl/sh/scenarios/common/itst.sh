#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# Trapping exit so we can remove the temp file used in ci run
trap clean_up EXIT

function clean_up() {
    local EXIT_CODE=$?
    local STDOUT
    local STDERR

    # Removes DEPLOY_LOG for ITST06/07
    if [ -f "$DEPLOY_LOG" ]; then
        log "Removing transfer tmp file..."
        rm -f "$DEPLOY_LOG"
    fi

    # Prints stderr of nodes on failures
    for i in $(seq 1 $(get_count_of_nodes)); do
        STDOUT=$(tail "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null) || true
        STDERR=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stderr.log 2>/dev/null) || true
        if [ ! -z "$STDERR" ]; then
            echo ""
            log "##############################################"
            log " Node-$i Error Outputs "
            log "##############################################"
            # true to ignore "No such file" error from cat
            echo ""
            log "STDERR:"
            echo "$STDERR"
            echo ""
            log "Tailed STDOUT:"
            echo "$STDOUT"
            echo ""
        fi
    done

    log "Test exited with exit code $EXIT_CODE"
    exit $EXIT_CODE
}

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
        nctl-status
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

# Gets the header of the switch block of the given era.
function get_switch_block() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local BLOCK_HASH=${3}
    local ERA=${4}

    if [ -z "$BLOCK_HASH" ]; then
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID"))
    else
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID") -b "$BLOCK_HASH")
    fi

    if [ "$WALKBACK" -gt 0 ]; then
        BLOCK=$(echo "$JSON_OUT" | jq '.result.block')
        local ERA_END="$(echo "$BLOCK" | jq '.header.era_end')"
        local ERA_ID="$(echo "$BLOCK" | jq '.header.era_id')"
        if [ "$ERA_END" = "null" ] || { [ -n "$ERA" ] && [ "$ERA_ID" != "$ERA" ]; }; then
            PARENT=$(echo "$BLOCK" | jq -r '.header.parent_hash')
            WALKBACK=$((WALKBACK - 1))
            get_switch_block "$NODE_ID" "$WALKBACK" "$PARENT" "$ERA"
        else
            echo "$BLOCK"
        fi
    else
        echo "null"
    fi
}

function get_switch_block_equivocators() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local BLOCK_HASH=${3}

    local BLOCK=$(get_switch_block "$NODE_ID" "$WALKBACK" "$BLOCK_HASH")

    if [ "$BLOCK" != "null" ]; then
        log "equivocators: $(echo "$BLOCK" | jq '.header.era_end.era_report.equivocators')"
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

function verify_wasm_inclusion() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local DEPLOY_HASH=${3}
    local BLOCK_HASH=${4}
    local JSON_OUT
    local PARENT
    local BLOCK_HEADER
    local BLOCK_DEPLOY_HASHES

    if [ -z "$BLOCK_HASH" ]; then
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID"))
    else
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID") -b "$BLOCK_HASH")
    fi

    if [ "$WALKBACK" -gt 0 ]; then
        BLOCK_HEADER=$(echo "$JSON_OUT" | jq '.result.block.header')
        BLOCK_DEPLOY_HASHES=$(echo "$JSON_OUT" | jq -r '.result.block.body.deploy_hashes[]')
        if grep -q "${DEPLOY_HASH}" <<< "$BLOCK_DEPLOY_HASHES"; then
            log "DEPLOY: $DEPLOY_HASH found in block!"
        else
            PARENT=$(echo "$BLOCK_HEADER" | jq -r '.parent_hash')
            WALKBACK=$((WALKBACK - 1))
            log "$WALKBACK: Walking back to block: $PARENT"
            verify_wasm_inclusion "$NODE_ID" "$WALKBACK" "$DEPLOY_HASH" "$PARENT"
        fi
    else
        log "Error: Deploy $DEPLOY_HASH not found within walkback!"
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
    local NODE_PATH=$(get_path_to_node "$NODE_ID")
    local PUBLIC_KEY_HEX=$(get_node_public_key_hex "$NODE_ID")
    local TIMEOUT=${2:-300}
    log_step "Waiting for a node-$NODE_ID to produce a block..."
    local OUTPUT=$(timeout "$TIMEOUT" tail -n 1 -f "$NODE_PATH/logs/stdout.log" | grep -o -m 1 "proposer: PublicKey::Ed25519($PUBLIC_KEY_HEX)")
    if ( echo "$OUTPUT" | grep -q "proposer: PublicKey::Ed25519($PUBLIC_KEY_HEX)" ); then
        log "Node-$NODE_ID created a block!"
        log "$OUTPUT"
    else
        log "ERROR: Node-$NODE_ID didn't create a block within timeout=$TIMEOUT"
        exit 1
    fi
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

function do_submit_auction_bids()
{
    local NODE_ID=${1}
    log_step "submitting POS auction bids:"
    log "----- ----- ----- ----- ----- -----"
    BID_AMOUNT="1000000000000000000000000000000"
    BID_DELEGATION_RATE=6

    source "$NCTL"/sh/contracts-auction/do_bid.sh \
            node="$NODE_ID" \
            amount="$BID_AMOUNT" \
            rate="$BID_DELEGATION_RATE" \
            quiet="TRUE"

    log "node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"

    log_step "awaiting 10 seconds for auction bid deploys to finalise"
    sleep 10.0
}
