#!/usr/bin/env bash
# -----------------------------------------------------------------
# Synopsis.
# -----------------------------------------------------------------

# Before 1.5.0 there wasn't an immediate switch block committed at
# genesis. Because historical sync relies on sync leaps to get the
# validators for a particular era, we encounter a special case
# when syncing the blocks of era 0 if they were created before 1.5.0
# because sync leap will not be able to include a switch block that
# has the validators for era 0.
# Since the auction delay is at least 1, we can generally rely on
# the fact that the validator set for era 1 and era 0 are the same.
#
# Test if a node that has synced back to some block in era 0 can
# continue syncing to genesis after it was restarted (and lost its
# validator matrix).

# Step 01: Start network from pre-built stage.
# Step 02: Await era-id >= 2.
# Step 03: Stage nodes 1-5 and upgrade.
# Step 04: Assert upgraded nodes 1-5.
# Step 05: Assert nodes 1-5 didn't stall.
# Step 06: Await 1 era.
# Step 07: Start node 6.
# Step 08: Wait for node 6 to sync back to a block in era 0.
# Step 09: Stop and restart node 6.
# Step 10: Wait for node 6 to sync to genesis.
# Step 11: Start node 7.
# Step 12: Wait for node 7 to sync back to the first block in era 1.
# Step 13: Stop and restart node 7.
# Step 14: Wait for node 7 to sync to genesis.
# Step 15: Terminate.

# ----------------------------------------------------------------
# Imports.
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/views/utils.sh"
source "$NCTL/sh/node/svc_$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Main entry point.
function _main()
{
    local STAGE_ID=${1}
    local INITIAL_PROTOCOL_VERSION
    local ACTIVATION_POINT
    local UPGRADE_HASH

    if [ ! -d "$(get_path_to_stage "$STAGE_ID")" ]; then
        log "ERROR :: stage $STAGE_ID has not been built - cannot run scenario"
        exit 1
    fi

    _step_01 "$STAGE_ID"
    _step_02

    # Set initial protocol version for use later.
    INITIAL_PROTOCOL_VERSION=$(get_node_protocol_version 1)
    # Establish consistent activation point for use later.
    ACTIVATION_POINT="$(get_chain_era)"
    # Get minimum era height
    MIN_ERA_HEIGHT=$(($(grep "minimum_era_height" "$(get_path_to_net)"/chainspec/chainspec.toml | cut -d'=' -f2)))

    _step_03 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_04 "$INITIAL_PROTOCOL_VERSION"
    _step_05
    _step_06
    _step_07
    _step_08
    _step_09
    _step_10
    _step_11
    _step_12
    _step_13
    _step_14
    _step_15
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}

    log_step_upgrades 0 "Begin upgrade_scenario_14"
    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" \
            stage="$STAGE_ID" \
    log "... Starting 5 validators"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await era-id >= 2.
function _step_02()
{
    log_step_upgrades 2 "awaiting until era 2"
    await_until_era_n 2
}

# Step 03: Stage nodes 1-6 and upgrade.
function _step_03()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}

    log_step_upgrades 3 "upgrading 1 thru 5 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    for i in $(seq 1 7); do
        log "... staging upgrade on validator node-$i"
        source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="$i" era="$ACTIVATION_POINT"
        echo ""
    done

    log "... awaiting 2 eras + 1 block"
    await_n_eras '2' 'true' '5.0' '2'
    await_n_blocks '1' 'true' '2'
}

# Step 04: Assert upgraded nodes 1-5.
function _step_04()
{
    local PROTOCOL_VERSION_INITIAL=${1}
    local NX_PROTOCOL_VERSION
    local NODE_ID

    log_step_upgrades 4 "Asserting nodes 1 thru 5 upgraded"

    # Assert nodes are running same protocol version.
    for NODE_ID in $(seq 1 5)
    do
        NX_PROTOCOL_VERSION=$(get_node_protocol_version "$NODE_ID")
        if [ "$NX_PROTOCOL_VERSION" = "$PROTOCOL_VERSION_INITIAL" ]; then
            log "ERROR :: upgrade failure :: nodes are not all running same protocol version"
            log "... Node $NODE_ID: $NX_PROTOCOL_VERSION = $PROTOCOL_VERSION_INITIAL"
            exit 1
        else
            log "Node $NODE_ID upgraded successfully: $PROTOCOL_VERSION_INITIAL -> $NX_PROTOCOL_VERSION"
        fi
    done
}

# Step 05: Assert nodes 1-5 didn't stall.
function _step_05()
{
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 5 "Asserting nodes 1 thru 5 didn't stall"

    HEIGHT_1=$(get_chain_height 2)
    await_n_blocks '5' 'true' '2'
    for NODE_ID in $(seq 1 5)
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -le "$HEIGHT_1" ]; then
            log "ERROR :: upgrade failure :: node-$NODE_ID has stalled"
            log " ... node-$NODE_ID : $HEIGHT_2 <= $HEIGHT_1"
            exit 1
        else
            log " ... no stall detected on node-$NODE_ID: $HEIGHT_2 > $HEIGHT_1 [expected]"
        fi
    done
}

# Step 06: Await 1 era.
function _step_06()
{
    log_step_upgrades 6 "awaiting 1 era"
    await_n_eras '1' 'true' '5.0' '2'
}

function start_node_with_latest_trusted_hash()
{
    local NODE_ID=${1}

    local LFB_HASH=$(render_last_finalized_block_hash "1" | cut -f2 -d= | cut -f2 -d ' ')
    do_start_node "$NODE_ID" "$LFB_HASH"
}

function wait_historical_sync_to_height()
{
    local NODE_ID=${1}
    local HEIGHT=${2}

    local LOW=$(get_node_lowest_available_block "$NODE_ID")
    local HIGH=$(get_node_highest_available_block "$NODE_ID")

    # First wait for node to start syncing
    while [ -z $HIGH ] || [ -z $LOW ] || [[ $HIGH -eq $LOW ]] || [[ $HIGH -eq 0 ]] || [[ $LOW -eq 0 ]]; do
        sleep 0.2
        LOW=$(get_node_lowest_available_block "$NODE_ID")
        HIGH=$(get_node_highest_available_block "$NODE_ID")
    done

    while [ -z $LOW ] || [[ $LOW -gt $HEIGHT ]]; do
        sleep 0.2
        LOW=$(get_node_lowest_available_block "$NODE_ID")
    done
}

# Step 07: Start node 6.
function _step_07()
{
    log_step_upgrades 7 "starting node 6"
    start_node_with_latest_trusted_hash "6"
}

# Step 08: Wait for node 6 to sync back to a block in era 0.
function _step_08()
{
    log_step_upgrades 8 "Wait for node 6 to sync back to a block in era 0"

    wait_historical_sync_to_height "6" "$(($MIN_ERA_HEIGHT-1))"
}

# Step 09: Stop and restart node 6.
function _step_09()
{
    log_step_upgrades 9 "Stopping and re-starting node 6"

    do_stop_node "6"
    sleep 2
    start_node_with_latest_trusted_hash "6"
}

# Step 10: Wait for node 6 to sync to genesis.
function _step_10()
{
    log_step_upgrades 10 "Waiting for node 6 to sync to genesis"
    await_node_historical_sync_to_genesis '6' '60'
}

# Step 11: Start node 7.
function _step_11()
{
    log_step_upgrades 11 "starting node 7"
    start_node_with_latest_trusted_hash "7"
}

# Step 12: Wait for node 7 to sync back to the first block in era 1.
function _step_12()
{
    log_step_upgrades 12 "Wait for node 7 to sync back to the first block in era 1"

    wait_historical_sync_to_height "7" "$(($MIN_ERA_HEIGHT+1))"
}

# Step 13: Stop and restart node 7.
function _step_13()
{
    log_step_upgrades 13 "Stopping and re-starting node 7"

    do_stop_node "7"
    sleep 2
    start_node_with_latest_trusted_hash "7"
}

# Step 14: Wait for node 7 to sync to genesis.
function _step_14()
{
    log_step_upgrades 14 "Waiting for node 7 to sync to genesis"
    await_node_historical_sync_to_genesis '7' '60'
}

# Step 15: Terminate.
function _step_15()
{
    log_step_upgrades 15 "upgrade_scenario_14 successful - tidying up"

    source "$NCTL/sh/assets/teardown.sh"

    log_break
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset _STAGE_ID
unset INITIAL_PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        stage) _STAGE_ID=${VALUE} ;;
        *)
    esac
done

_main "${_STAGE_ID:-1}"
