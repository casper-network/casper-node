#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/assets/upgrade.sh
source "$NCTL"/sh/scenarios/common/itst.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that performs an emergency restart on the network.
# It also simulates social consensus on replacing the original validators (nodes 1-5)
# with a completely new set (nodes 6-10).
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing. Default=300 seconds.
#   `version=X_Y_Z` new protocol version to upgrade to. Default=2_0_0.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Emergency upgrade test begins"
    log "------------------------------------------------------------"

    do_await_genesis_era_to_complete

    # 1. Send batch of Wasm deploys
    do_send_wasm_deploys
    # 2. Send batch of native transfers
    do_send_transfers
    # 3. Wait until they're all included in the chain.
    do_await_deploy_inclusion
    # 4. Stop the network for the emergency upgrade.
    do_stop_network
    # 5. Prepare the nodes for the upgrade.
    do_prepare_upgrade
    # 6. Restart the network (start both the old and the new validators).
    do_restart_network
    # 7. Wait for the network to upgrade.
    do_await_network_upgrade
    # 8. Send batch of Wasm deploys
    do_send_wasm_deploys
    # 9. Send batch of native transfers
    do_send_transfers
    # 10. Wait until they're all included in the chain.
    do_await_deploy_inclusion
    # 11. Run Health Checks
    # ... restarts=15: due to node being stopped and started
    # ... crashes=5: expected in an emergency restart scenario?
    # ... errors=ignore: ticket sre issue 77
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='ignore' \
            equivocators=0 \
            doppels=0 \
            crashes=5 \
            restarts=15 \
            ejections=0

    log "------------------------------------------------------------"
    log "Emergency upgrade test ends"
    log "------------------------------------------------------------"
}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    log "------------------------------------------------------------"
    STEP=$((STEP + 1))
}

function do_await_genesis_era_to_complete() {
    log_step "awaiting genesis era to complete"
    while [ "$(get_chain_era)" != "1" ]; do
        sleep 1.0
    done
}

function do_stop_network() {
    log_step "stopping the network for an emergency upgrade"
    ACTIVATE_ERA="$(get_chain_era)"
    log "emergency upgrade activation era = $ACTIVATE_ERA"
    local ERA_ID=$((ACTIVATE_ERA - 1))
    local BLOCK=$(get_switch_block "1" "32" "" "$ERA_ID")
    # read the latest global state hash
    STATE_HASH=$(echo "$BLOCK" | jq -r '.header.state_root_hash')
    # save the LFB hash to use as the trusted hash for the restart
    TRUSTED_HASH=$(echo "$BLOCK" | jq -r '.hash')
    log "state hash = $STATE_HASH"
    log "trusted hash = $TRUSTED_HASH"
    # stop the network
    do_node_stop_all
}

function do_prepare_upgrade() {
    log_step "preparing the network emergency upgrade to version ${PROTOCOL_VERSION} at era ${ACTIVATE_ERA}"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)"); do
        _emergency_upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID" "$STATE_HASH" 1 "$(get_count_of_genesis_nodes)"
    done
}

function do_restart_network() {
    log_step "restarting the network: starting both old and new validators"
    # start the network
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)"); do
        do_node_start "$NODE_ID" "$TRUSTED_HASH"
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
    await_n_eras 1
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

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset PROTOCOL_VERSION
STEP=0

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        timeout) SYNC_TIMEOUT_SEC=${VALUE} ;;
        version) PROTOCOL_VERSION=${VALUE} ;;
        *) ;;
    esac
done

SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}
PROTOCOL_VERSION=${PROTOCOL_VERSION:-"2_0_0"}

main
