#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that runs a network network, puts a load on it and validates if it remains stable.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Network soundnses test start"
    log "------------------------------------------------------------"

    # do_await_genesis_era_to_complete

    start_sending_deploys_periodically 10 10

    sleep 500

    log "------------------------------------------------------------"
    log "Network soundnses test complete"
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
    while [ "$(get_chain_era)" != "1" ]; do
        sleep 1.0
    done
}

function start_sending_deploys_periodically() {
    log_step "starting background process to send deploys"
    local BATCH_COUNT=${1}
    local INTERVAL=${2}
    local DEPLOY_SENDER_PID=$!

    bash -i /home/magister/Casper/casper-node/utils/nctl/sh/scenarios/common/abc.sh &
    

    #coproc bkgndProc {
    #    bash -i /home/magister/Casper/casper-node/utils/nctl/sh/scenarios/common/abc.sh
    #}

    log "background process pid=${DEPLOY_SENDER_PID}"
}

function do_send_wasm_deploys() {
    # NOTE: Maybe make these arguments to the test?
    local BATCH_COUNT=1
    local BATCH_SIZE=1
    local TRANSFER_AMOUNT=2500000000
    log_step "sending Wasm deploys"
    # prepare wasm batch
    prepare_wasm_batch "$TRANSFER_AMOUNT" "$BATCH_COUNT" "$BATCH_SIZE"
    # dispatch wasm batches
    for BATCH_ID in $(seq 1 $BATCH_COUNT); do
        dispatch_wasm_batch "$BATCH_ID"
    done
}

function do_send_transfers() {
    log_step "sending native transfers"
    # NOTE: Maybe make these arguments to the test?
    local AMOUNT=2500000000
    local TRANSFERS_COUNT=5
    local NODE_ID="random"

    # Enumerate set of users.
    for USER_ID in $(seq 1 "$(get_count_of_users)"); do
        dispatch_native "$AMOUNT" "$USER_ID" "$TRANSFERS_COUNT" "$NODE_ID"
    done
}

function do_await_deploy_inclusion() {
    # Should be enough to await for one era.
    log_step "awaiting one era…"
    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='180'
}

function do_read_lfb_hash() {
    local NODE_ID=${1}
    LFB_HASH=$(render_last_finalized_block_hash "$NODE_ID" | cut -f2 -d= | cut -f2 -d ' ')
    echo "$LFB_HASH"
}

function do_start_new_node() {
    local NODE_ID=${1}
    local SYNC_TO_GENESIS_MODE=${2}
    local CONFIG_PATH

    CONFIG_PATH="$(find $(get_path_to_node $NODE_ID) -name config.toml)"

    log_step "starting new node-$NODE_ID. Syncing from hash=${LFB_HASH}"
    export RUST_LOG="info,casper_node::components::linear_chain_sync=trace"

    if [ ! -z "$SYNC_TO_GENESIS_MODE" ]; then
        sed -i "s/sync_to_genesis =.*/sync_to_genesis = $SYNC_TO_GENESIS_MODE/g" "$CONFIG_PATH"
    fi
    log "Sync-to-genesis mode: $(cat $CONFIG_PATH | grep 'sync_to_genesis')"

    # TODO: Do not hardcode.
    do_node_start "$NODE_ID" "$LFB_HASH"
}

function do_await_full_synchronization() {
    local NODE_ID=${1}
    local WAIT_TIME_SEC=0
    log_step "awaiting full synchronization of the new node=${NODE_ID}…"
    while [ "$(do_read_lfb_hash "$NODE_ID")" != "$(do_read_lfb_hash 1)" ]; do
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to synchronize in ${SYNC_TIMEOUT_SEC} seconds"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
    # Wait one more era and then test LFB again.
    # This way we can verify that the node is up-to-date with the protocol state
    # after transitioning to an active validator.
    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='180'
    while [ "$(do_read_lfb_hash "$NODE_ID")" != "$(do_read_lfb_hash 1)" ]; do
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to keep up with the protocol state"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
}

function dispatch_native() {
    local AMOUNT=${1}
    local USER_ID=${2}
    local TRANSFERS=${3}
    local NODE_ID=${4}

    source "$NCTL"/sh/contracts-transfers/do_dispatch_native.sh amount="$AMOUNT" \
        user="$USER_ID" \
        transfers="$TRANSFERS" \
        node="$NODE_ID"
}

function dispatch_wasm_batch() {
    local BATCH_ID=${1:-1}
    local INTERVAL=${2:-0.01}
    local NODE_ID=${3:-"random"}

    source "$NCTL"/sh/contracts-transfers/do_dispatch_wasm_batch.sh batch="$BATCH_ID" \
        interval="$INTERVAL" \
        node="$NODE_ID"
}

function prepare_wasm_batch() {
    local AMOUNT=${1}
    local BATCH_COUNT=${2}
    local BATCH_SIZE=${3}

    source "$NCTL"/sh/contracts-transfers/do_prepare_wasm_batch.sh amount="$AMOUNT" \
        count="$BATCH_COUNT" \
        size="$BATCH_SIZE"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TO_GENESIS_NODE_ID
unset FAST_SYNC_NODE_ID
unset SYNC_TIMEOUT_SEC
unset LFB_HASH
STEP=0

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        sync_to_genesis_node) SYNC_TO_GENESIS_NODE_ID=${VALUE} ;;
        fast_sync_node) FAST_SYNC_NODE_ID=${VALUE} ;;
        timeout) SYNC_TIMEOUT_SEC=${VALUE} ;;
        *) ;;
    esac
done

SYNC_TO_GENESIS_NODE_ID=${SYNC_TO_GENESIS_NODE_ID:-"6"}
FAST_SYNC_NODE_ID=${FAST_SYNC_NODE_ID:-"7"}
SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}

main "$SYNC_TO_GENESIS_NODE_ID" "$FAST_SYNC_NODE_ID"
