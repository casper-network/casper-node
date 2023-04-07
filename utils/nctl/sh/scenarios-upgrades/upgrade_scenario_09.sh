#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# Step 01: Start network from pre-built stage.
# Step 02: Await era-id >= 2.
# Step 03: Stage nodes 1-5 and upgrade.
# Step 04: Assert upgraded nodes 1-5.
# Step 05: Assert nodes 1-5 didn't stall.
# Step 06: Await 1 era and stage nodes 6&7 with a recent trusted hash (pre & post).
# Step 07: Await 7 eras and stage nodes 8&9 with an old trusted hash (pre & post).
# Step 08: Verify that nodes 6&7 successfully syncs and nodes 8&9 did not sync as expected.
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
    ACTIVATION_POINT="$(($(get_chain_era) + NCTL_DEFAULT_ERA_ACTIVATION_OFFSET))"
    PRE_UPGRADE_BLOCK_HASH="$($(get_path_to_client) get-block --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"

    _step_03 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_04 "$INITIAL_PROTOCOL_VERSION"

    POST_UPGRADE_BLOCK_HASH="$($(get_path_to_client) get-block --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"

    _step_05
    _step_06 "$PRE_UPGRADE_BLOCK_HASH" "$POST_UPGRADE_BLOCK_HASH"
    _step_07 "$PRE_UPGRADE_BLOCK_HASH" "$POST_UPGRADE_BLOCK_HASH"
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

# Step 06: Await 1 era and then start nodes 6&7 with trusted hash within the allowed range
function _step_06()
{
    local PRE_UPGRADE_BLOCK_HASH=${1}
    local POST_UPGRADE_BLOCK_HASH=${2}
    log_step_upgrades 6 "awaiting 1 era to start node 6 & 7"
    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='300' node_id='2'
    _stage_node_with_trusted_hash "6" "$STAGE_ID" "$ACTIVATION_POINT" "$PRE_UPGRADE_BLOCK_HASH"
    _stage_node_with_trusted_hash "7" "$STAGE_ID" "$ACTIVATION_POINT" "$POST_UPGRADE_BLOCK_HASH"
}

# Step 07: Await 7 eras and then start nodes 8&9 with a trusted hash that is too old
function _step_07()
{
    local PRE_UPGRADE_BLOCK_HASH=${1}
    local POST_UPGRADE_BLOCK_HASH=${2}
    log_step_upgrades 7 "awaiting 7 eras for block hashes to become too old"
    nctl-await-n-eras offset='7' sleep_interval='5.0' timeout='600' node_id='2'
    _stage_node_with_trusted_hash "8" "$STAGE_ID" "$ACTIVATION_POINT" "$PRE_UPGRADE_BLOCK_HASH"
    _stage_node_with_trusted_hash "9" "$STAGE_ID" "$ACTIVATION_POINT" "$POST_UPGRADE_BLOCK_HASH"
}

# Stage node with a trusted hash
function _stage_node_with_trusted_hash()
{
    local NODE_ID=${1}
    local STAGE_ID=${2}
    local ACTIVATION_POINT=${3}
    local BLOCK_HASH=${4}
    local PATH_TO_NODE_CONFIG_UPGRADE
    local N2_PROTO_VERSION

    # Node 2 would be running the upgrade if we made it this far in the test.
    # sed is for switching from: ie. 1.0.0 -> 1_0_0
    N2_PROTO_VERSION="$(get_node_protocol_version 2 | sed 's/\./_/g')"

    log "upgrading node $NODE_ID from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" \
        stage="$STAGE_ID" \
        verbose=false \
        node="$NODE_ID" \
        era="$ACTIVATION_POINT"
    echo ""

    source "$NCTL/sh/node/start.sh" node="$NODE_ID" hash="$BLOCK_HASH"
}

# Step 08: Assert nodes 1-7 didn't stall
function _step_08()
{
    # give nodes 8 and 9 time to fail
    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='300' node_id='2'

    while [ "$(get_count_of_up_nodes)" != '7' ]; do
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

    log_step_upgrades 8 "Asserting nodes 1 thru 7 didn't stall"

    HEIGHT_1=$(get_chain_height 2)
    await_n_blocks '5' 'true' '2'
    for NODE_ID in $(seq 1 7)
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
    log_step_upgrades 8 "Asserting nodes 8 & 9 are not running"
    if [ "$(get_node_is_up 8)" = true ] || [ "$(get_node_is_up 9)" = true ]; then
        log "ERROR :: upgrade failure :: a node is running using an out of range trusted hash"
        exit 1
    else
        log " ... nodes 8 & 9 are not running [expected]"
    fi
}

# Step 10: Run NCTL health checks
function _step_10()
{
    # restarts=5 - Nodes that upgrade
    # errors=2 - node 8 & 9: "fatal error via control announcement"
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
