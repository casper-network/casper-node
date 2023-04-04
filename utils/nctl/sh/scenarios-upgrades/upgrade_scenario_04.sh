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
# Step 11: Check Reactor State
# Step 12: Assert lfbs are in sync
# Step 13: Assert chain didn't stall.
# Step 14: Run Health Checks
# Step 15: Terminate.

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
    ACTIVATION_POINT="$(($(get_chain_era) + NCTL_DEFAULT_ERA_ACTIVATION_OFFSET))"

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
    _step_14
    _step_15
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

    log_step_upgrades 0 "Begin upgrade_scenario_04"
    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
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

    do_await_genesis_era_to_complete 'false'
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
    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='180' node_id='2'
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
        source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" \
            stage="$STAGE_ID" \
            verbose=false \
            node="$i" \
            era="$ACTIVATION_POINT"
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

# Step 11: Check reactor state
function _step_11()
{
    local FIRST_NODE=1
    local LAST_NODE=10
    local TIMEOUT=180
    local TIME_COUNT=0
    local NODE_INDEX
    local NODE_REACTOR_STATE
    local ALLOWED_STATES

    ALLOWED_STATES=('Validate' 'KeepUp')
    NODE_INDEX="$FIRST_NODE"

    log_step_upgrades 11 "Check reactor states..."

    while [ "$NODE_INDEX" -le "$LAST_NODE" ] && [ "$TIME_COUNT" -lt "$TIMEOUT" ]; do
        NODE_REACTOR_STATE=$(get_reactor_state "$NODE_INDEX")
        if [[ "${ALLOWED_STATES[@]}" =~ "$NODE_REACTOR_STATE" ]]; then
            log "Node-$NODE_INDEX found with reactor state of: $NODE_REACTOR_STATE [ok]"
            NODE_INDEX=$((NODE_INDEX + 1))
        else
            log "Node-$NODE_INDEX found with reactor state of: $NODE_REACTOR_STATE [retrying]"
            log "... time remaining until timeout: $((TIMEOUT - TIME_COUNT))"
            TIME_COUNT=$((TIME_COUNT + 1))
            sleep 1
        fi
    done

    # gt since the index would be incremented to 11 if it made it to 10
    if [ "$NODE_INDEX" -gt "$LAST_NODE" ]; then
        log "... all nodes reactor states ok!"
    else
        log "Error: reactor check timed out after $TIMEOUT seconds"
        exit 1
    fi
}

# Step 12: Assert lfbs are in sync
function _step_12()
{
    log_step_upgrades 12 "Asserting all nodes are in sync..."
    # args: first node, last node, timeout, log_step
    check_network_sync '1' '10' '300' 'false'
}

# Step 13: Assert chain didn't stall.
function _step_13()
{
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 13 "Asserting nodes didn't stall"

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

# Step 14: Run NCTL health checks
function _step_14()
{
    # restarts=12 - Nodes that upgrade
    log_step_upgrades 14 "running health checks"
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='0' \
            equivocators='0' \
            doppels='0' \
            crashes=0 \
            restarts=12 \
            ejections=0
}

# Step 15: Terminate.
function _step_15()
{
    log_step_upgrades 15 "upgrade_scenario_04 successful - tidying up"

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
