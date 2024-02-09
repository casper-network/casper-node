#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/assets/upgrade.sh
source "$NCTL"/sh/scenarios/common/itst.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that performs an emergency restart on the network,
# and then also performs a regular upgrade after.
# It also simulates social consensus on replacing the original validators (nodes 1-5)
# with a completely new set (nodes 6-10).
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing. Default=300 seconds.
#   `version=X_Y_Z` new protocol version to upgrade to. Default=2_0_0.
#   `version2=X_Y_Z` new protocol version to upgrade to. Default=3_0_0.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Upgrade after emergency upgrade test pre-1.5 begins"
    log "------------------------------------------------------------"

    do_prepare_stage

    # Start the network
    do_start_network
    # 0. Await for the completion of the genesis era
    do_await_genesis_era_to_complete
    # 1. Send batch of Wasm deploys
    do_send_wasm_deploys
    # 2. Send batch of native transfers
    do_send_transfers
    # 3. Wait until they're all included in the chain.
    do_await_deploy_inclusion '1'
    # 4. Stop the network for the emergency upgrade.
    do_stop_network
    # 5. Prepare the nodes for the emergency upgrade.
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
    do_await_deploy_inclusion '6'
    # 11. Prepare the second upgrade.
    do_upgrade_second_time
    # 12. Await the second upgrade.
    do_await_second_upgrade
    # 13. Reset node 1 to sync from scratch.
    do_reset_node_1
    # 14. Wait until node 1 syncs back to genesis.
    await_node_historical_sync_to_genesis '1' "$SYNC_TIMEOUT_SEC"
    # 15. Run Health Checks
    # ... restarts=26: due to nodes being stopped and started; node 1 3 times, nodes 2-5 2 times,
    # ................ node 6-10 3 times (start at 1.4.8, restart to 1.4.7 for sync, then 2 upgrades)
    # ... errors: ignore pattern due to non-deterministic error messages "could not send response
    # .................. to request down oneshot channel"
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='0;ignore:to request down oneshot channel' \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=26 \
            ejections=0

    log "------------------------------------------------------------"
    log "Upgrade after emergency upgrade test ends"
    log "------------------------------------------------------------"

    nctl-assets-teardown
}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    log "------------------------------------------------------------"
    STEP=$((STEP + 1))
}

function do_prepare_stage() {
    local PATH_TO_STAGE=${1}
    local STARTING_VERSION=${2}
    local INCREMENT
    local RC_VERSION

    log "... removing stray remotes and stages"
    rm -rf $(get_path_to_stages)
    rm -rf $(get_path_to_remotes)

    log "... setting remote 1.4.7"

    nctl-stage-set-remotes "1.4.7"

    log "... setting remote 1.4.8"

    nctl-stage-set-remotes "1.4.8"

    log "... preparing settings"

    mkdir -p "$(get_path_to_stage '1')"

    cat <<EOF > "$(get_path_to_stage_settings 1)"
export NCTL_STAGE_SHORT_NAME="YOUR-SHORT-NAME"

export NCTL_STAGE_DESCRIPTION="YOUR-DESCRIPTION"

export NCTL_STAGE_TARGETS=(
    "1_4_7:remote"
    "$TEST_PROTOCOL_VERSION:remote"
    "$TEST_PROTOCOL_VERSION_2:local"
)
EOF

    log "... building stage from settings"

    nctl-stage-build-from-settings
}

function do_start_network() {
    nctl-assets-setup-from-stage stage=1
    nctl-start
    sleep 10
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

# A 1.4-compatible version of the function
function _generate_global_state_update() {
    local PROTOCOL_VERSION=${1}
    local STATE_HASH=${2}
    local STATE_SOURCE=${3:-1}
    local NODE_COUNT=${4:-5}

    local PATH_TO_NET=$(get_path_to_net)
    local TOOL_PATH="$(get_path_to_stage '1')"/"$PROTOCOL_VERSION"

    if [ -f "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml ]; then
        # global state update file exists, no need to generate it again
        return
    fi

    local STATE_SOURCE_PATH=$(get_path_to_node $STATE_SOURCE)

    # Create parameters to the global state update generator.
    # First, we supply the path to the directory of the node whose global state we'll use
    # and the trusted hash.
    local PARAMS
    PARAMS="validators -d ${STATE_SOURCE_PATH}/storage/$(get_chain_name) -s ${STATE_HASH}"

    # Add the parameters that define the new validators.
    # We're using the reserve validators, from NODE_COUNT+1 to NODE_COUNT*2.
    local PATH_TO_NODE
    local PUBKEY
    for NODE_ID in $(seq $((NODE_COUNT + 1)) $((NODE_COUNT * 2))); do
        PATH_TO_NODE=$(get_path_to_node $NODE_ID)
        PUBKEY=`cat "$PATH_TO_NODE"/keys/public_key_hex`
        PARAMS="${PARAMS} -v ${PUBKEY},$(($NODE_ID + 1000000000000000))"
    done

    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"

    # Create the global state update file.
    "$TOOL_PATH"/global-state-update-gen $PARAMS \
        > "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml
}

function do_prepare_upgrade() {
    log_step "preparing the network emergency upgrade to version ${TEST_PROTOCOL_VERSION} at era ${ACTIVATE_ERA}"
    nctl-assets-upgrade-from-stage stage="1" era="$ACTIVATE_ERA"

    local PATH_TO_NET=$(get_path_to_net)

    _generate_global_state_update "$TEST_PROTOCOL_VERSION" "$STATE_HASH" "1" "$(get_count_of_genesis_nodes)"

    for NODE_ID in $(seq 1 "$(get_count_of_nodes)"); do
        local PATH_TO_NODE=$(get_path_to_node $NODE_ID)

        # Specify hard reset in the chainspec.
        local SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_NODE/config/$TEST_PROTOCOL_VERSION/chainspec.toml');"
            "cfg['protocol']['hard_reset']=True;"
            "cfg['protocol']['last_emergency_restart']=$ACTIVATE_ERA;"
            "toml.dump(cfg, open('$PATH_TO_NODE/config/$TEST_PROTOCOL_VERSION/chainspec.toml', 'w'));"
        )
        python3 -c "${SCRIPT[*]}"

        cp "$PATH_TO_NET"/chainspec/"$TEST_PROTOCOL_VERSION"/global_state.toml \
            "$PATH_TO_NODE"/config/"$TEST_PROTOCOL_VERSION"/global_state.toml

        # remove stored state of the launcher - this will make the launcher start from the highest
        # available version instead of from the previously executed one
        if [ -e "$PATH_TO_NODE"/config/casper-node-launcher-state.toml ]; then
            rm "$PATH_TO_NODE"/config/casper-node-launcher-state.toml
        fi
    done
}

function do_restart_network() {
    log_step "restarting the network: starting both old and new validators"
    # start the network
    # do not pass the trusted hash to the original validators - they already have all the blocks
    # and will simply pick up at the upgrade point
    for NODE_ID in $(seq 1 5); do
        do_node_start "$NODE_ID"
    done
    # the new validators need the trusted hash in order to be able to sync to the network, as they
    # don't have any blocks at this point
    for NODE_ID in $(seq 6 "$(get_count_of_nodes)"); do
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
    local NODE_ID=${1}
    # Should be enough to await for one era.
    log_step "awaiting one era…"
    nctl-await-n-eras node_id=$NODE_ID offset='1' sleep_interval='5.0' timeout='180'
}

function do_upgrade_second_time() {
    ACTIVATE_ERA_2=$(($(get_chain_era)+2))
    log_step "scheduling the network upgrade to version ${TEST_PROTOCOL_VERSION_2} at era ${ACTIVATE_ERA_2}"

    CONFIG_PATH="$NCTL/overrides/upgrade_after_emergency_upgrade_test_pre_1_5.config.toml"
    nctl-assets-upgrade-from-stage stage="1" era="$ACTIVATE_ERA_2" config_path="$CONFIG_PATH"
}

function do_await_second_upgrade() {
    log_step "wait for the network to upgrade"
    local WAIT_TIME_SEC=0
    local WAIT_UNTIL=$((ACTIVATE_ERA_2 + 1))
    while [ "$(get_chain_era)" != "$WAIT_UNTIL" ]; do
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to upgrade the network in ${SYNC_TIMEOUT_SEC} seconds"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
}

function do_reset_node_1() {
    log_step "resetting and restarting node 1"
    do_node_stop "1"
    local PATH_TO_NODE=$(get_path_to_node "1")
    local PATH_TO_STORAGE="${PATH_TO_NODE}/storage/$(get_chain_name)"
    # remove all storage, so that the node will start fresh
    rm -r "$PATH_TO_STORAGE"/*
    # use node 7 for the latest block hash
    TRUSTED_HASH=$(get_chain_latest_block_hash "7")
    do_node_start "1" "$TRUSTED_HASH"
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
unset TEST_PROTOCOL_VERSION
unset TEST_PROTOCOL_VERSION_2
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
TEST_PROTOCOL_VERSION="1_4_8"
TEST_PROTOCOL_VERSION_2="1_5_0"

main
