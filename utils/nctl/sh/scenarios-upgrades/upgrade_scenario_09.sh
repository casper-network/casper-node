#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# Step 01: Start network from pre-built stage.
# Step 02: Await era-id >= 2.
# Step 03: Stage nodes 1-5 and upgrade.
# Step 04: Assert upgraded nodes 1-5.
# Step 05: Assert nodes 1-5 didn't stall.
# Step 06: Await 1 era.
# Step 07: Stage node-6 with old trusted hash.
# Step 08: Verify that node-6 successfully syncs.
# Step 09: Run Health Checks
# Step 10: Terminate.

# ----------------------------------------------------------------
# Imports.
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/views/utils.sh"
source "$NCTL/sh/node/svc_$NCTL_DAEMON_TYPE".sh
source "$NCTL/sh/scenarios/common/itst.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Main entry point.
function _main()
{
    local STAGE_ID=${1}
    local INITIAL_PROTOCOL_VERSION
    local ACTIVATION_POINT
    local PRE_UPGRADE_BLOCK_HASH
    local POST_UPGRADE_BLOCK_HASH

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
    PRE_UPGRADE_BLOCK_HASH="$($(get_path_to_client) get-block --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"

    _step_03 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_04 "$INITIAL_PROTOCOL_VERSION"

    POST_UPGRADE_BLOCK_HASH="$($(get_path_to_client) get-block --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"

    _step_05
    _step_06
    _step_07 "$STAGE_ID" "$ACTIVATION_POINT" "$PRE_UPGRADE_BLOCK_HASH" "$POST_UPGRADE_BLOCK_HASH"
    _step_08
    _step_09
    _step_10
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}
    local PATH_TO_STAGE
    local PATH_TO_PROTO1

    PATH_TO_STAGE=$(get_path_to_stage "$STAGE_ID")
    pushd "$PATH_TO_STAGE"
    PATH_TO_PROTO1=$(ls -d */ | sort | head -n 1 | tr -d '/')
    popd

    log_step_upgrades 0 "Begin upgrade_scenario_09"
    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
    log "... Starting 5 validators"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await era-id >= 2.
function _step_02()
{
    log_step_upgrades 2 "awaiting until era 2"

    do_wait_until_era '2' 'false' '180'
}

# Step 03: Stage nodes 1-5 and upgrade.
function _step_03()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}

    log_step_upgrades 3 "upgrading 1 thru 5 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    for i in $(seq 1 5); do
        log "... staging upgrade on validator node-$i"
        source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" \
            stage="$STAGE_ID" \
            verbose=false \
            node="$i" \
            era="$ACTIVATION_POINT"
        echo ""
    done

    log "... awaiting 2 eras + 1 block"
    nctl-await-n-eras offset='2' sleep_interval='5.0' timeout='180' node_id='2'
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
    log_step_upgrades 6 "awaiting next era"
    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='180' node_id='2'
}

# Step 07: Stage node 6 with post-upgrade trusted hash and node 7 with pre-upgrade trusted hash
function _step_07()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}
    local PRE_UPGRADE_BLOCK_HASH=${3}
    local POST_UPGRADE_BLOCK_HASH=${4}
    local PATH_TO_NODE_CONFIG_UPGRADE
    local N2_PROTO_VERSION

    # Node 2 would be running the upgrade if we made it this far in the test.
    # sed is for switching from: ie. 1.0.0 -> 1_0_0
    N2_PROTO_VERSION="$(get_node_protocol_version 2 | sed 's/\./_/g')"

    log_step_upgrades 7 "upgrading nodes 6 and 7 from stage ($STAGE_ID) - expecting node 7 to fail"

    log "... setting upgrade assets"

    source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" \
        stage="$STAGE_ID" \
        verbose=false \
        node="6" \
        era="$ACTIVATION_POINT"
    echo ""
    source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" \
        stage="$STAGE_ID" \
        verbose=false \
        node="7" \
        era="$ACTIVATION_POINT"
    echo ""

    source "$NCTL/sh/node/start.sh" node='6' hash="$POST_UPGRADE_BLOCK_HASH"
    source "$NCTL/sh/node/start.sh" node='7' hash="$PRE_UPGRADE_BLOCK_HASH"
}

# Step 08: Assert nodes 1-6 didn't stall
function _step_08()
{
    while [ "$(get_count_of_up_nodes)" != '6' ]; do
        sleep 1.0
        SLEEP_COUNT=$((SLEEP_COUNT + 1))
        log "NODE_COUNT_UP: $(get_count_of_up_nodes)"
        log "Sleep time: $SLEEP_COUNT seconds"

        if [ "$SLEEP_COUNT" -ge "60" ]; then
            log "Timeout reached of 1 minute! Exiting ..."
            exit 1
        fi
    done

    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 8 "Asserting nodes 1 thru 6 didn't stall"

    HEIGHT_1=$(get_chain_height 2)
    await_n_blocks '5' 'true' '2'
    for NODE_ID in $(seq 1 6)
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

# Step 09: Assert node 7 is not running
function _step_09()
{
    log_step_upgrades 8 "Asserting node 7 is not running"
    if [ "$(get_node_is_up 7)" = true ]; then
        log "ERROR :: upgrade failure :: node-7 is running using a pre-upgrade trusted hash"
        exit 1
    else
        log " ... node-7 not running [expected]"
    fi
}

# Step 10: Run NCTL health checks
function _step_10()
{
    # restarts=5 - Nodes that upgrade
    # errors=2 - node 7: "failed to sync linear chain" and "fatal error via control announcement"
    log_step_upgrades 10 "running health checks"
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='2' \
            equivocators='0' \
            doppels='0' \
            crashes=0 \
            restarts=5 \
            ejections=0
}

# Step 11: Terminate.
function _step_11()
{
    log_step_upgrades 11 "upgrade_scenario_09 successful - tidying up"

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
