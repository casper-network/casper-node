#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# - Start nodes 1-5 running version V1
# - Execute some deploys to populate global state a little
# - Upgrade nodes 1-5 to V2
# - Assert V2 nodes run & the chain advances (new blocks are generated)
# - Stage upgrade of node 6 to V2
# - Start the node 6, assert that it upgrades
# - Assert all nodes run & the chain advances (new blocks are generated)
# - Clean-up

# ----------------------------------------------------------------
# Imports.
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/node/svc_$NCTL_DAEMON_TYPE".sh

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Main entry point.
function _main()
{
    local STAGE_ID=${1}

    if [ ! -d $(get_path_to_stage "$STAGE_ID") ]; then
        log "ERROR :: stage $STAGE_ID has not been built - cannot run scenario"
        exit 1
    fi


    _step_01 "$STAGE_ID"
    _step_02

    local INITIAL_PROTOCOL_VERSION=$(get_node_protocol_version 1)
    local ACTIVATION_POINT="$(get_chain_era)"

    _step_03
    _step_04
    _step_05 "$STAGE_ID" "$ACTIVATION_POINT"

    _copy_new_client_binary "$STAGE_ID"

    _step_06 "$INITIAL_PROTOCOL_VERSION"
    _step_07 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_08 "$INITIAL_PROTOCOL_VERSION"
    _step_09
    _step_10
}

function _copy_new_client_binary()
{
    local STAGE_ID=${1}
    local PATH_TO_STAGE
    local PATH_TO_STAGE_SETTINGS
    local HIGHEST_VERSION_AND_TYPE
    local HIGHEST_VERSION
    local UPGRADED_CLIENT_PATH
    local CLIENT_PATH

    # Source the settings.sh file.
    PATH_TO_STAGE="$(get_path_to_stage $STAGE_ID)"
    PATH_TO_STAGE_SETTINGS="$PATH_TO_STAGE/settings.sh"
    source "$PATH_TO_STAGE_SETTINGS"

    # Read the last line - will be e.g. "1_5_0:local".
    HIGHEST_VERSION_AND_TYPE="${NCTL_STAGE_TARGETS[-1]}"

    # Extract the version from the line.
    IFS=':' read -ra SPLIT_LINE <<< "$HIGHEST_VERSION_AND_TYPE"
    HIGHEST_VERSION="${SPLIT_LINE[0]}"

    UPGRADED_CLIENT_PATH="$PATH_TO_STAGE/$HIGHEST_VERSION/casper-client"
    CLIENT_PATH="$(get_path_to_client)"
    log "Replacing client binary at $CLIENT_PATH with $UPGRADED_CLIENT_PATH"

    cp "$UPGRADED_CLIENT_PATH" "$CLIENT_PATH"
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}

    log_step_upgrades 1 "starting nodes (nodes 1-5) from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
    for i in {1..5}
    do
        source "$NCTL/sh/node/start.sh" node=$i
    done
}

# Step 02: Await era-id >= 1.
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"
    await_until_era_n 1
}

# Step 03: Populate global state -> native + wasm transfers.
function _step_03()
{
    log_step_upgrades 3 "dispatching deploys to populate global state"

    log "... 100 native transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_native.sh" \
        transfers=100 interval=0.0 verbose=false

    log "... 100 wasm transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_wasm.sh" \
        transfers=100 interval=0.0 verbose=false
}

# Step 04: Await era-id += 1.
function _step_04()
{
    log_step_upgrades 4 "awaiting next era"

    await_n_eras 1
}

# Step 05: Upgrade nodes 1-5 from stage.
function _step_05()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}

    log_step_upgrades 5 "upgrading 1 thru 5 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    for i in $(seq 1 5); do
        log "... staging upgrade on node-$i"
        source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="$i" era="$ACTIVATION_POINT" force_launcher_upgrade=false
        echo ""
    done

    log "... awaiting 2 eras + 1 block"
    await_n_eras '2'
    await_n_blocks '1'
}

# Step 06: Assert chain is progressing at all nodes.
function _step_06()
{
    local N1_PROTOCOL_VERSION_INITIAL=${1}
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 6 "asserting nodes 1-5 upgrade"

    # Assert no nodes have stopped.
    if [ "$(get_count_of_up_nodes)" != "$(get_count_of_genesis_nodes)" ]; then
        log "ERROR :: protocol upgrade failure - >= 1 nodes have stopped"
        exit 1
    fi

    # Assert no nodes have stalled.
    HEIGHT_1=$(get_chain_height)
    await_n_blocks 2
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -le "$HEIGHT_1" ]; then
            log "ERROR :: protocol upgrade failure - >= 1 nodes have stalled"
            exit 1
        fi
    done

    # Assert node-1 protocol version incremented.
    N1_PROTOCOL_VERSION=$(get_node_protocol_version 1)
    if [ "$N1_PROTOCOL_VERSION" == "$N1_PROTOCOL_VERSION_INITIAL" ]; then
        log "ERROR :: protocol upgrade failure - >= protocol version did not increment"
        exit 1
    else
        log "Node 1 upgraded successfully: $N1_PROTOCOL_VERSION_INITIAL -> $N1_PROTOCOL_VERSION"
    fi

    # Assert nodes are running same protocol version.
    for NODE_ID in $(seq 2 "$(get_count_of_genesis_nodes)")
    do
        NX_PROTOCOL_VERSION=$(get_node_protocol_version "$NODE_ID")
        if [ "$NX_PROTOCOL_VERSION" != "$N1_PROTOCOL_VERSION" ]; then
            log "ERROR :: protocol upgrade failure - >= nodes are not all running same protocol version"
            exit 1
        else
            log "Node $NODE_ID upgraded successfully: $N1_PROTOCOL_VERSION_INITIAL -> $NX_PROTOCOL_VERSION"
        fi
    done
}

# Step 07: Stage update of the passive node 6
function _step_07()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}
    local TRUSTED_HASH="$(get_chain_latest_block_hash)"

    log_step_upgrades 7 "upgrading node 6 from stage ($STAGE_ID)"

    log "... setting upgrade assets"

    log "... staging upgrade on node-6"
    source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" stage="$STAGE_ID" verbose=false node="6" era="$ACTIVATION_POINT" force_launcher_upgrade=true
    echo ""

    log "... starting node-6"
    do_node_start "6" "$TRUSTED_HASH" true

    log "... awaiting 2 eras + 1 block"
    await_n_eras '2'
    await_n_blocks '1'
}

# Step 08: Assert node 6 is upgraded
function _step_08
{
    local N1_PROTOCOL_VERSION_INITIAL=${1}
   
    log_step_upgrades 8 "asserting node 6 upgrade"

    # Assert no nodes have stalled.
    HEIGHT_1=$(get_chain_height)
    await_n_blocks 2
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -le "$HEIGHT_1" ]; then
            log "ERROR :: protocol upgrade failure - >= 1 nodes have stalled"
            exit 1
        fi
    done

    local N6_PROTOCOL_VERSION=$(get_node_protocol_version 6)
    if [ -z "$N6_PROTOCOL_VERSION" ]; then
        log "ERROR :: protocol upgrade failure - >= unable to read protocol version of node 6"
        exit 1
    fi
    if [ "$N6_PROTOCOL_VERSION" == "$N1_PROTOCOL_VERSION_INITIAL" ]; then
        log "ERROR :: protocol upgrade failure - >= protocol version did not increment"
        exit 1
    else
        log "Node 6 upgraded successfully: $N1_PROTOCOL_VERSION_INITIAL -> $N6_PROTOCOL_VERSION"
    fi
}

# Step 09: Assert passive joined & all are running upgrade.
function _step_09()
{
    local N1_PROTOCOL_VERSION_INITIAL=${1}
    local NODE_ID
    local N1_BLOCK_HASH
    local N1_PROTOCOL_VERSION
    local N1_STATE_ROOT_HASH
    local NX_PROTOCOL_VERSION
    local NX_STATE_ROOT_HASH
    local RETRY_COUNT

    log_step_upgrades 9 "asserting all nodes are running upgrade"

    # Assert all nodes are live.
    for NODE_ID in $(seq 1 "$(get_count_of_up_nodes)")
    do
        if [ $(get_node_is_up "$NODE_ID") == false ]; then
            log "ERROR :: protocol upgrade failure - >= 1 nodes not live"
            exit 1
        fi
    done

    # Assert node-1 protocol version incremented.
    N1_PROTOCOL_VERSION=$(get_node_protocol_version 1)
    if [ "$N1_PROTOCOL_VERSION" == "$N1_PROTOCOL_VERSION_INITIAL" ]; then
        log "ERROR :: protocol upgrade failure - >= protocol version did not increment"
        log "ERROR :: Initial Proto Version: $N1_PROTOCOL_VERSION_INITIAL"
        log "ERROR :: Current Proto Version: $N1_PROTOCOL_VERSION"
        exit 1
    else
        log "Node 1 upgraded successfully: $N1_PROTOCOL_VERSION_INITIAL -> $N1_PROTOCOL_VERSION"
    fi

    # Assert nodes are running same protocol version.
    for NODE_ID in $(seq 2 "$(get_count_of_up_nodes)")
    do
        NX_PROTOCOL_VERSION=$(get_node_protocol_version "$NODE_ID")
        if [ "$NX_PROTOCOL_VERSION" != "$N1_PROTOCOL_VERSION" ]; then
            log "ERROR :: protocol upgrade failure - >= nodes are not all running same protocol version"
            exit 1
        else
            log "Node $NODE_ID upgraded successfully: $N1_PROTOCOL_VERSION_INITIAL -> $NX_PROTOCOL_VERSION"
        fi
    done

    # Assert nodes are synced.
    N1_BLOCK_HASH=$(get_chain_latest_block_hash 1)
    N1_STATE_ROOT_HASH=$(get_state_root_hash 1 "$N1_BLOCK_HASH")
    for NODE_ID in $(seq 2 "$(get_count_of_up_nodes)")
    do
        # Retry command to address: https://github.com/casper-network/casper-node/issues/1499
        unset NX_STATE_ROOT_HASH
        unset RETRY_COUNT
        RETRY_COUNT=1
        while [ -z "$NX_STATE_ROOT_HASH" ] || [ "$NX_STATE_ROOT_HASH" = 'null' ]; do
            if [ "$RETRY_COUNT" -gt 10 ]; then
                log "ERROR :: NODE-$NODE_ID RETRY :: Failed to get state root hash within 10 retries"
                exit 1
            else
                NX_STATE_ROOT_HASH=$(get_state_root_hash "$NODE_ID" "$N1_BLOCK_HASH")
                sleep 1
                ((RETRY_COUNT=RETRY_COUNT+1))
            fi
        done

        if [ "$NX_STATE_ROOT_HASH" != "$N1_STATE_ROOT_HASH" ]; then
            log "ERROR :: protocol upgrade failure - >= nodes are not all at same root hash"
            log "ERROR :: BLOCK HASH = $N1_BLOCK_HASH"
            log "ERROR :: $NODE_ID  :: ROOT HASH = $NX_STATE_ROOT_HASH :: N1 ROOT HASH = $N1_STATE_ROOT_HASH"
            exit 1
        else
            log "HASH MATCH :: $NODE_ID  :: HASH = $NX_STATE_ROOT_HASH :: N1 HASH = $N1_STATE_ROOT_HASH"
        fi
    done
}

# Step 10: Terminate.
function _step_10()
{
    log_step_upgrades 10 "test successful - tidying up"

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
