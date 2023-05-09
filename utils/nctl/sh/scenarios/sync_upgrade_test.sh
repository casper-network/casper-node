#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/assets/upgrade.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that tries to sync a new node
# with an upgraded network.
#
# Arguments:
#   `node=XXX` ID of a new node. Default=6
#   `timeout=XXX` timeout (in seconds) when syncing. Default=300 seconds.
#   `version=X_Y_Z` new protocol version to upgrade to. Default=2_0_0.
#   `era=X` at which the upgrade should take place. Default=3.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Syncing node begins"
    log "------------------------------------------------------------"

    do_await_genesis_era_to_complete

    # 1. Send batch of Wasm deploys
    do_send_wasm_deploys
    # 2. Send batch of native transfers
    do_send_transfers
    # 3. Wait until they're all included in the chain.
    do_await_deploy_inclusion
    # 4. Upgrade the network
    do_upgrade_network
    # 5. Wait for the network to upgrade.
    do_await_network_upgrade
    assert_network_upgrade "$PROTOCOL_VERSION"
    # 6. Take a note of the last finalized block hash
    do_read_lfb_hash
    # 7. Send batch of Wasm deploys
    do_send_wasm_deploys
    # 8. Send batch of native transfers
    do_send_transfers
    # 9. Wait until they're all included in the chain.
    do_await_deploy_inclusion
    # 10. Start the node in sync mode using hash from 4)
    do_start_new_node "$NEW_NODE_ID"
    # 11. Wait until it's synchronized
    #     and verify that its last finalized block matches other nodes'.
    do_await_full_synchronization "$NEW_NODE_ID"
    # 12. Run Closing Health Checks
    # ... restarts=6: due to nodes being stopped and started
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=5 \
            ejections=0

    log "------------------------------------------------------------"
    log "Syncing node complete"
    log "------------------------------------------------------------"
}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    log "------------------------------------------------------------"
    STEP=$((STEP + 1))
}

function do_upgrade_network() {
    log_step "scheduling the network upgrade to version ${PROTOCOL_VERSION} at era ${ACTIVATE_ERA}"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)"); do
        _upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID"
    done
}

function do_await_network_upgrade() {
    log_step "wait for the network to upgrade"
    local WAIT_TIME_SEC=0
    local WAIT_UNTIL=$((ACTIVATE_ERA + 1))
    while [ "$(get_chain_era)" != "$WAIT_UNTIL" ]; do
    if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
        log "ERROR: Failed to upgrade the network in ${SYNC_TIMEOUT_SEC} seconds"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
}

function assert_network_upgrade() {
    local STATUS
    local COUNT
    local RUNNING_COUNT
    local PROTO=${1}
    local CONVERTED
    log_step "checking that entire network upgraded to $PROTO"
    CONVERTED=$(echo $PROTO | sed 's/_/./g')
    STATUS=$(nctl-view-node-status)
    COUNT=$(grep 'api_version' <<< $STATUS[*] | grep -o "$CONVERTED" | wc -l)
    RUNNING_COUNT=$(get_running_node_count)

    if [ ! "$COUNT" = "$RUNNING_COUNT" ]; then
        log "ERROR: Upgrade failed, $COUNT out of $RUNNING_COUNT upgraded successfully."
        exit 1
    fi
    log "$COUNT out of $RUNNING_COUNT upgraded successfully!"
}

function do_send_wasm_deploys() {
    # NOTE: Maybe make these arguments to the test?
    local BATCH_COUNT=1
    local BATCH_SIZE=1
    local TRANSFER_AMOUNT=10000
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
    log_step "starting new node-$NODE_ID. Syncing from hash=${LFB_HASH}"
    export RUST_LOG="info,casper_node::components::linear_chain_sync=trace"
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

unset NEW_NODE_ID
unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset ACTIVATE_ERA
unset PROTOCOL_VERSION
STEP=0

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        node) NEW_NODE_ID=${VALUE} ;;
        timeout) SYNC_TIMEOUT_SEC=${VALUE} ;;
        era) ACTIVATE_ERA=${VALUE} ;;
        version) PROTOCOL_VERSION=${VALUE} ;;
        *) ;;
    esac
done

NEW_NODE_ID=${NEW_NODE_ID:-"6"}
SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}
ACTIVATE_ERA=${ACTIVATE_ERA:-"5"}
PROTOCOL_VERSION=${PROTOCOL_VERSION:-"2_0_0"}

main
