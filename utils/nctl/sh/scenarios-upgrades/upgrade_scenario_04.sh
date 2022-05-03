#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# Step 01: Start network from pre-built stage.
# Step 02: Await era-id >= 1.
# Step 03: Stage nodes 2-9 and upgrade.
# Step 04: Assert upgraded nodes 2-9.
# Step 05: Assert nodes 1&10 didn't upgrade.
# Step 06: Assert nodes 2-9 didn't stall.
# Step 07: Assert nodes 1&10 did stall.
# Step 08: Await 1 era.
# Step 09: Stage nodes 1&10 and restart.
# Step 10: Assert all nodes are running
# Step 11: Assert lfbs are in sync
# Step 12: Assert chain didn't stall.
# Step 13: Terminate.

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
    _step_05 "$INITIAL_PROTOCOL_VERSION"
    _step_06
    _step_07
    _step_08

    # Workaround for https://github.com/casper-network/casper-node/pull/2101#issuecomment-923205726
    if [ "$(echo $INITIAL_PROTOCOL_VERSION | tr -d '.')" -ge "140" ]; then
        log "... using latest block hash (post version 1.4.0) [expected]"
        UPGRADE_HASH="$($(get_path_to_client) get-block --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"
    else
        log "... using block 1 hash (pre version 1.4.0) [expected]"
        UPGRADE_HASH="$($(get_path_to_client) get-block -b 1 --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"
    fi

    _step_09 "$STAGE_ID" "$ACTIVATION_POINT" "$UPGRADE_HASH"
    _step_10
    _step_11
    _step_12
    _step_13
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}

    log_step_upgrades 0 "Begin upgrade_scenario_04"
    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" \
            stage="$STAGE_ID" \
            chainspec_path="$NCTL/sh/scenarios/chainspecs/upgrade_scenario_4.chainspec.toml.in"
    log "... Starting 5 validators"
    source "$NCTL/sh/node/start.sh" node=all
    log "... Starting 5 non-validators"
    for i in $(seq 6 10); do
        source "$NCTL/sh/node/start.sh" node="$i"
    done
}

# Step 02: Await era-id >= 1.
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"
    await_until_era_n 1
}

# Step 03: Stage nodes 2-9 and upgrade.
function _step_03()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}

    log_step_upgrades 3 "upgrading 2 thru 9 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    for i in $(seq 2 9); do
        if [ "$i" -le '5' ]; then
            log "... staging upgrade on validator node-$i"
        else
            log "... staging upgrade on non-validator node-$i"
        fi
        source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="$i" era="$ACTIVATION_POINT"
        echo ""
    done

    log "... awaiting 2 eras + 1 block"
    await_n_eras '2' 'true' '5.0' '2'
    await_n_blocks '1' 'true' '2'
}

# Step 04: Assert upgraded nodes 2-9.
function _step_04()
{
    local PROTOCOL_VERSION_INITIAL=${1}
    local NX_PROTOCOL_VERSION
    local NODE_ID

    log_step_upgrades 4 "Asserting nodes 2 thru 9 upgraded"

    # Assert nodes are running same protocol version.
    for NODE_ID in $(seq 2 9)
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

# Step 05: Assert nodes 1&10 didn't upgrade.
function _step_05()
{
    local PROTOCOL_VERSION_INITIAL=${1}
    local NX_PROTOCOL_VERSION
    local NODE_ID

    log_step_upgrades 5 "Asserting nodes 1 and 10 didn't upgrade"

    # Assert nodes are running same protocol version.
    for NODE_ID in 1 10
    do
        NX_PROTOCOL_VERSION=$(get_node_protocol_version "$NODE_ID")
        if [ "$NX_PROTOCOL_VERSION" != "$PROTOCOL_VERSION_INITIAL" ]; then
            log "ERROR :: failure :: nodes are not all running same protocol version"
            log "... Node $NODE_ID: $NX_PROTOCOL_VERSION != $PROTOCOL_VERSION_INITIAL"
            exit 1
        else
            log "Node $NODE_ID didn't upgrade: $PROTOCOL_VERSION_INITIAL = $NX_PROTOCOL_VERSION [expected]"
        fi
    done
}

# Step 06: Assert nodes 2-9 didn't stall.
function _step_06()
{
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 6 "Asserting nodes 2 thru 9 didn't stall"

    HEIGHT_1=$(get_chain_height 2)
    await_n_blocks '5' 'true' '2'
    for NODE_ID in $(seq 2 9)
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

# Step 07: Assert nodes 1&10 did stall.
function _step_07()
{
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 7 "Asserting nodes 1 and 10 stalled"

    HEIGHT_1=$(get_chain_height 1)
    await_n_blocks '5' 'true' '2'

    for NODE_ID in 1 10
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -ne "$HEIGHT_1" ]; then
            log "ERROR :: upgrade failure :: node-$NODE_ID didn't stall"
            exit 1
        else
            log " ... stall detected on node-$NODE_ID: $HEIGHT_2 = $HEIGHT_1 [expected]"
        fi
    done
}

# Step 08: Await 1 era.
function _step_08()
{
    log_step_upgrades 8 "awaiting next era"
    await_n_eras '1' 'true' '5.0' '2'
}

# Step 09: Stage nodes 1&10 and restart.
function _step_09()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}
    local HASH=${3}
    local PATH_TO_NODE_CONFIG_UPGRADE
    local N2_PROTO_VERSION

    # Node 2 would be running the upgrade if we made it this far in the test.
    # sed is for switching from: ie. 1.0.0 -> 1_0_0
    N2_PROTO_VERSION="$(get_node_protocol_version 2 | sed 's/\./_/g')"

    log_step_upgrades 9 "upgrading nodes 1&10 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    for i in 1 10; do
        if [ "$i" -le '5' ]; then
            log "... staging upgrade on validator node-$i"
        else
            log "... staging upgrade on non-validator node-$i"
        fi
        source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="$i" era="$ACTIVATION_POINT"
        echo ""
        # add hash to upgrades config
        PATH_TO_NODE_CONFIG_UPGRADE="$(get_path_to_node_config $i)/$N2_PROTO_VERSION/config.toml"
        _update_node_config_on_start "$PATH_TO_NODE_CONFIG_UPGRADE" "$HASH"
    done

    log "... restarting nodes 1 & 10"
    source "$NCTL/sh/node/stop.sh" node='1'
    sleep 1
    source "$NCTL/sh/node/stop.sh" node='10'
    sleep 5
    source "$NCTL/sh/node/start.sh" node='10' hash="$HASH"
    sleep 1
    source "$NCTL/sh/node/start.sh" node='1' hash="$HASH"
    sleep 5
}

# Step 10: Assert all nodes are running
function _step_10()
{
    local RUNNING_COUNT

    log_step_upgrades 10 "Asserting all nodes are running..."

    # true in case of bad grep which would exit 1
    RUNNING_COUNT="$(nctl-status | grep -c 'RUNNING' || true)"
    if [ "$RUNNING_COUNT" != '10' ]; then
        log "ERROR: $RUNNING_COUNT of 10 nodes found running"
        log "... dumping logs"
        nctl-status; nctl-assets-dump
        exit 1
    else
        log "... $RUNNING_COUNT of 10 nodes found running [expected]"
    fi
}

# Step 11: Assert lfbs are in sync
function _step_11()
{
    local NODES_IN_SYNC
    local LOOPS_ALLOWED

    # allow 5 minutes
    LOOPS_ALLOWED=300

    log_step_upgrades 11 "Asserting all nodes are in sync..."

    while [ "$LOOPS_ALLOWED" != '0' ]; do
        NODES_IN_SYNC=$(nctl-view-chain-lfb | awk '{print $NF}' | uniq -c | sed 's/^ *//g' | awk '{ print $1 }')
        if [ "$NODES_IN_SYNC" = '10' ]; then
            log "... all $NODES_IN_SYNC nodes are in sync"
            break
        else
            # Retry sync check for 20m in 10s intervals
            LOOPS_ALLOWED=$(( LOOPS_ALLOWED - 10 ))
            log "...retrying for another $((LOOPS_ALLOWED / 60)) mins, sleep 10"
            sleep 10
        fi
    done

    if [ "$LOOPS_ALLOWED" = '0' ]; then
        log "ERROR: 10 nodes not found in sync"
        log "... dumping logs"
        nctl-view-chain-height; nctl-assets-dump
        exit 1
    fi
}

# Step 12: Assert chain didn't stall.
function _step_12()
{
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 12 "Asserting nodes didn't stall"

    HEIGHT_1=$(get_chain_height 2)
    await_n_blocks '5' 'true' '2'
    for NODE_ID in $(seq 1 10)
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -le "$HEIGHT_1" ]; then
            log "ERROR :: upgrade failure :: node-$NODE_ID has stalled"
            log " ... $HEIGHT_2 <= $HEIGHT_1"
            exit 1
        else
            log " ... no stall detected on node-$NODE_ID: $HEIGHT_2 > $HEIGHT_1 [expected]"
        fi
    done
}

# Step 13: Terminate.
function _step_13()
{
    log_step_upgrades 13 "upgrade_scenario_04 successful - tidying up"

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
