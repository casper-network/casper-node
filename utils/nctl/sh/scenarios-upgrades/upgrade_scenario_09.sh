#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# Step 01: Start network from pre-built stage.
# Step 02: Await era-id >= 1.
# Step 03: Stage nodes 1-5 and upgrade.
# Step 04: Assert upgraded nodes 1-5.
# Step 05: Assert nodes 1-5 didn't stall.
# Step 06: Await 1 era.
# Step 07: Stage node-6 with old trusted hash.
# Step 08: Verify failed node-6.
# Step 09: Terminate.

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
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}

    log_step_upgrades 0 "Begin upgrade_scenario_09"
    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" \
            stage="$STAGE_ID"
    log "... Starting 5 validators"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await era-id >= 1.
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"
    await_until_era_n 1
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
    HASH="$($(get_path_to_client) get-block -b 1 --node-address "$(get_node_address_rpc '2')" | jq -r '.result.block.hash')"

    # Node 2 would be running the upgrade if we made it this far in the test.
    # sed is for switching from: ie. 1.0.0 -> 1_0_0
    N2_PROTO_VERSION="$(get_node_protocol_version 2 | sed 's/\./_/g')"

    log_step_upgrades 7 "upgrading node 6 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="6" era="$ACTIVATION_POINT"
    echo ""
    # add hash to upgrades config
    PATH_TO_NODE_CONFIG_UPGRADE="$(get_path_to_node_config $i)/$N2_PROTO_VERSION/config.toml"
    _update_node_config_on_start "$PATH_TO_NODE_CONFIG_UPGRADE" "$HASH"

    source "$NCTL/sh/node/start.sh" node='6' hash="$HASH"
}

# Step 08: Check for expected failure.
function _step_08()
{
    local NODE_ID=${1}
    local NODE_PATH
    local LOG_MSG

    LOG_MSG='the trusted block has an older version'

    log_step_upgrades 8 "Checking for failed node-$NODE_ID..."

    NODE_PATH="$(get_path_to_node $NODE_ID)"

    if ( cat "$NODE_PATH"/logs/stdout.log | grep -q "$LOG_MSG" ); then
        log "...Message Found - '$LOG_MSG' [expected]"
    else
        log "ERROR: Message Not Found - '$LOG_MSG'"
        exit 1
    fi

    if ( nctl-status | grep "node-$NODE_ID" | grep -q 'EXITED' ); then
        log "...node-$NODE_ID found stopped [expected]"
    else
        log "ERROR: node-$NODE_ID found still running!"
        exit 1
    fi
}

# Step 09: Terminate.
function _step_09()
{
    log_step_upgrades 9 "upgrade_scenario_09 successful - tidying up"

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
