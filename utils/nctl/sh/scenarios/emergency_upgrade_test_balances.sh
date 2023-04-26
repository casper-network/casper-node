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
# It also simulates social consensus on transferring some funds from the user-1 account to an
# account corresponding to the key 010101...01.
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing. Default=300 seconds.
#   `version=X_Y_Z` new protocol version to upgrade to. Default=2_0_0.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Emergency upgrade test balances begins"
    log "------------------------------------------------------------"

    do_await_genesis_era_to_complete

    local SRC_KEY=$(get_account_key "user" 1)
    local SRC_PURSE=$(get_main_purse_uref ${SRC_KEY})
    local INITIAL_SRC_BALANCE=$(get_account_balance ${SRC_PURSE})

    log "Src key = ${SRC_KEY}"
    log "Src purse = ${SRC_PURSE}"
    log "Initial src balance = ${INITIAL_SRC_BALANCE}"

    # 1. Wait until they're all included in the chain.
    do_await_one_era
    # 2. Stop the network for the emergency upgrade.
    do_stop_network
    # 3. Prepare the nodes for the upgrade.
    do_prepare_upgrade
    # 4. Restart the network (start both the old and the new validators).
    do_restart_network
    # 5. Wait for the network to upgrade.
    do_await_network_upgrade
    # 6. Wait until they're all included in the chain.
    do_await_one_era

    local TGT_KEY="010101010101010101010101010101010101010101010101010101010101010101"
    local TGT_PURSE=$(get_main_purse_uref ${TGT_KEY})
    local FINAL_SRC_BALANCE=$(get_account_balance ${SRC_PURSE})
    local FINAL_TGT_BALANCE=$(get_account_balance ${TGT_PURSE})

    local SRC_DIFF=$((INITIAL_SRC_BALANCE - FINAL_SRC_BALANCE))

    log "Transferred $TRANSFER_AMOUNT"
    log "Source: final balance $FINAL_SRC_BALANCE, diff $SRC_DIFF"
    log "Target: final balance $FINAL_TGT_BALANCE"

    if [ $FINAL_TGT_BALANCE -ne $TRANSFER_AMOUNT ]; then
        log "FAILED: target final balance differs from the transfer amount."
        exit 1
    fi

    if [ $SRC_DIFF -ne $TRANSFER_AMOUNT ]; then
        log "FAILED: source balance changed by a value different from the transfer amount."
        exit 1
    fi

    # 7. Run Closing Health Checks
    # ... restarts=10: due to nodes being stopped and started
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=5 \
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

    local ACCOUNT_KEY=$(get_account_key "user" 1)
    local SRC_ACC="account-hash-$(get_account_hash ${ACCOUNT_KEY})"
    local TGT_KEY="010101010101010101010101010101010101010101010101010101010101010101"
    local TGT_ACC="account-hash-$(get_account_hash ${TGT_KEY})"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)"); do
        _emergency_upgrade_node_balances "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID" "$STATE_HASH" 1 "$SRC_ACC" "$TGT_ACC" "$TRANSFER_AMOUNT"
    done
}

function do_restart_network() {
    log_step "restarting the network: starting both old and new validators"
    # start the network
    for NODE_ID in $(seq 1 "$(get_count_of_genesis_nodes)"); do
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

function do_await_one_era() {
    # Should be enough to await for one era.
    log_step "awaiting one era…"
    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='180'
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset PROTOCOL_VERSION
STEP=0
TRANSFER_AMOUNT="1000000000"

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
