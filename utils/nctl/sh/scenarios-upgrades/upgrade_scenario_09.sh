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

    _step_03 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_04 "$INITIAL_PROTOCOL_VERSION"
    _step_05
    _step_06
    _step_07 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_08 '6'
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

    source "$NCTL/sh/assets/setup_from_stage.sh" \
            stage="$STAGE_ID" \
            chainspec_path="$PATH_TO_STAGE/$PATH_TO_PROTO1/upgrade_chainspecs/upgrade_scenario_9.chainspec.toml.in"
    log "... Starting 5 validators"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await era-id >= 2.
function _step_02()
{
    log_step_upgrades 2 "awaiting until era 2"
    await_until_era_n 2
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
        source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="$i" era="$ACTIVATION_POINT" chainspec_path="$NCTL/sh/scenarios/chainspecs/upgrade_scenario_9.chainspec.toml.in"
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
    log_step_upgrades 6 "awaiting next era"
    await_n_eras '1' 'true' '5.0' '2'
}

# Step 07: Stage node 6 with old trusted hash.
function _step_07()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}
    local HASH
    local PATH_TO_NODE_CONFIG_UPGRADE
    local N2_PROTO_VERSION

    # Use trusted hash prior to upgrade point
    # Block 31 should be in era 2 or 3 - so before the upgrade, but after era 0, which is important
    # because a node can't get the validators set for a block in era 0.
    HASH="$($(get_path_to_client) get-block -b 31 --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"

    # Node 2 would be running the upgrade if we made it this far in the test.
    # sed is for switching from: ie. 1.0.0 -> 1_0_0
    N2_PROTO_VERSION="$(get_node_protocol_version 2 | sed 's/\./_/g')"

    log_step_upgrades 7 "upgrading node 6 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="6" era="$ACTIVATION_POINT" chainspec_path="$NCTL/sh/scenarios/chainspecs/upgrade_scenario_9.chainspec.toml.in"
    echo ""
    # add hash to upgrades config
    PATH_TO_NODE_CONFIG_UPGRADE="$(get_path_to_node_config $i)/$N2_PROTO_VERSION/config.toml"
    _update_node_config_on_start "$PATH_TO_NODE_CONFIG_UPGRADE" "$HASH"

    source "$NCTL/sh/node/start.sh" node='6' hash="$HASH"
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

# Step 09: Run NCTL health checks
function _step_09()
{
    # restarts=5 - Nodes that upgrade
    log_step_upgrades 9 "running health checks"
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='0' \
            equivocators='0' \
            doppels='0' \
            crashes=0 \
            restarts=5 \
            ejections=0
}

# Step 10: Terminate.
function _step_10()
{
    log_step_upgrades 10 "upgrade_scenario_09 successful - tidying up"

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
