#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# Step 01: Start network from pre-built stage.
# Step 02: Await era-id >= 2.
# Step 03: Stage nodes 1-5 and upgrade.
# Step 04: Assert upgraded nodes 1-5.
# Step 05: Assert nodes 1-5 didn't stall.
# Step 06: Await 2 eras.
# Step 07: Check rpc backwards compatibility for the
#          chain_get_era_info_by_switch_block and chain_get_era_summary RPC endpoints
# Step 08: Terminate.

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

    _step_03 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_04 "$INITIAL_PROTOCOL_VERSION"
    _step_05
    _step_06
    _step_07
    _step_08
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}

    log_step_upgrades 0 "Begin upgrade_scenario_13"
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

# Step 06: Await 2 eras.
function _step_06()
{
    log_step_upgrades 6 "awaiting 2 eras"
    await_n_eras '2' 'true' '5.0' '2'
}

function _compare_era_summary()
{
    local BLOCK_HASH=${1}
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl "1")

    local ERA_SUMMARY_RPC_RESPONSE=$(
        curl -s --header 'Content-Type: application/json' \
            --request POST "$NODE_ADDRESS_CURL" \
            --data-raw "{
                \"jsonrpc\": \"2.0\",
                \"method\": \"chain_get_era_summary\",
                \"params\": {
                    \"block_identifier\": {
                        \"Hash\": $BLOCK_HASH
                    }
                },
                \"id\": 1
            }" | jq '.result.era_summary'
    )

    local ERA_INFO_RPC_RESPONSE=$(
        curl -s --header 'Content-Type: application/json' \
            --request POST "$NODE_ADDRESS_CURL" \
            --data-raw "{
                \"jsonrpc\": \"2.0\",
                \"method\": \"chain_get_era_info_by_switch_block\",
                \"params\": {
                    \"block_identifier\": {
                        \"Hash\": $BLOCK_HASH
                    }
                },
                \"id\": 1
            }" | jq '.result.era_summary'
    )

    if [ "$ERA_SUMMARY_RPC_RESPONSE" = "null" ]; then
        log "ERROR :: chain_get_era_summary failed with block hash $BLOCK_HASH"
        exit 1
    fi

    if [ "$ERA_INFO_RPC_RESPONSE" = "null" ]; then
        log "ERROR :: chain_get_era_info_by_switch_block failed with block hash $BLOCK_HASH"
        exit 1
    fi

    if [ "$ERA_SUMMARY_RPC_RESPONSE" != "$ERA_INFO_RPC_RESPONSE" ]; then
        log "ERROR :: era summary does not match for block hash $BLOCK_HASH"
        exit 1
    fi
}

# Check rpc backwards compatibility for the chain_get_era_info_by_switch_block RPC
function _step_07()
{
    log_step_upgrades 7 "comparing era summary obtained through chain_get_era_info_by_switch_block and chain_get_era_summary"

    local LATEST_SWITCH_BLOCK=$(get_switch_block "1" "32" "" "")
    local LATEST_SWITCH_BLOCK_HASH=$(echo $LATEST_SWITCH_BLOCK | jq '.hash')
    _compare_era_summary $LATEST_SWITCH_BLOCK_HASH

    # check both RPCs still work for switch blocks before the upgrade
    local SWITCH_BLOCK_BEFORE_UPGRADE=$(get_switch_block "1" "150" "" "2")
    local SWITCH_BLOCK_BEFORE_UPGRADE_HASH=$(echo $SWITCH_BLOCK_BEFORE_UPGRADE | jq '.hash')
    _compare_era_summary $SWITCH_BLOCK_BEFORE_UPGRADE_HASH
}

# Step 08: Terminate.
function _step_08()
{
    log_step_upgrades 8 "upgrade_scenario_13 successful - tidying up"

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
