#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# 1. Start network from pre-built stage.
# 2. Execute some deploys to populate global state a little
# 3. Upgrade all running nodes to v2
# 4. Assert v2 nodes run & the chain advances (new blocks are generated)
# 5. Start passive nodes.  The launcher should cause the v1 node to run, it should exit and the v2 node should then catch up.
# 6. Assert passive nodes are now running.

# ----------------------------------------------------------------
# Imports.
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/node/svc_$NCTL_DAEMON_TYPE".sh
source "$NCTL/sh/scenarios/common/itst.sh"

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

    # Set initial protocol version for use later.
    INITIAL_PROTOCOL_VERSION=$(get_node_protocol_version 1)
    _step_03
    _step_04
    _step_05 "$STAGE_ID"
    _step_06 "$INITIAL_PROTOCOL_VERSION"
    _step_07
    _step_08 "$INITIAL_PROTOCOL_VERSION"
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

    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await for genesis
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"

    do_await_genesis_era_to_complete 'false'
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

    nctl-await-n-eras offset='1' sleep_interval='2.0' timeout='180'
}

# Step 05: Upgrade network from stage.
function _step_05()
{
    local STAGE_ID=${1}

    log_step_upgrades 5 "upgrading network from stage ($STAGE_ID)"

    log "... setting upgrade assets"
    source "$NCTL/sh/assets/upgrade_from_stage.sh" \
        stage="$STAGE_ID" \
        verbose=false

    log "... awaiting 2 eras + 1 block"
    nctl-await-n-eras offset='2' sleep_interval='5.0' timeout='180'
    await_n_blocks 1
}

# Step 06: Assert chain is progressing at all nodes.
function _step_06()
{
    local N1_PROTOCOL_VERSION_INITIAL=${1}
    local TIMEOUT=${2:-'60'}
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 6 "asserting node upgrades"

    while [ "$TIMEOUT" -ge 0 ]; do
        # Assert no nodes have stopped.
        if [ "$(get_count_of_up_nodes)" != "$(get_count_of_genesis_nodes)" ]; then
            if [ "$TIMEOUT" != '0' ]; then
                log "...waiting for nodes to come up, timeout=$TIMEOUT"
                sleep 1
                TIMEOUT=$((TIMEOUT-1))
            else
                log "ERROR :: protocol upgrade failure - >= 1 nodes have stopped"
                exit 1
            fi
        else
            log "... all nodes up! [continuing]"
            break
        fi
    done

    # Assert no nodes have stalled.
    HEIGHT_1=$(get_chain_height)
    await_n_blocks 2
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -le "$HEIGHT_1" ]; then
            log "ERROR :: protocol upgrade failure - node $NODE_ID has stalled - current height $HEIGHT_2 is <= starting height $HEIGHT_1"
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

# Step 07: Join passive nodes.
function _step_07()
{
    local NODE_ID
    local TRUSTED_HASH
    local SLEEP_COUNT

    log_step_upgrades 7 "joining passive nodes"

    log "... submitting auction bids"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        if [ $(get_node_is_up "$NODE_ID") == false ]; then
            source "$NCTL/sh/contracts-auction/do_bid.sh" \
                node="$NODE_ID" \
                amount="$(get_node_staking_weight "$NODE_ID")" \
                rate="2" \
                quiet="TRUE"
        fi
    done

    log "... awaiting auction bid acceptance (2 eras + 1 block)"
    nctl-await-n-eras offset='2' sleep_interval='5.0' timeout='180'
    await_n_blocks 1

    log "... starting nodes"
    TRUSTED_HASH="$(get_chain_latest_block_hash)"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        if [ $(get_node_is_up "$NODE_ID") == false ]; then
            do_node_start "$NODE_ID" "$TRUSTED_HASH"
        fi
    done

    log "... ... awaiting new nodes to start"

    while [ "$(get_count_of_up_nodes)" != '10' ]; do
        sleep 1.0
        SLEEP_COUNT=$((SLEEP_COUNT + 1))
        log "NODE_COUNT_UP: $(get_count_of_up_nodes)"
        log "Sleep time: $SLEEP_COUNT seconds"

        if [ "$SLEEP_COUNT" -ge "60" ]; then
            log "Timeout reached of 1 minute! Exiting ..."
            exit 1
        fi
    done

    nctl-await-n-eras offset='1' sleep_interval='5.0' timeout='180'
    await_n_blocks 1
}

# Step 08: Assert passive joined & all are running upgrade.
function _step_08()
{
    local N1_PROTOCOL_VERSION_INITIAL=${1}
    local NODE_ID
    local N1_BLOCK_HASH
    local N1_PROTOCOL_VERSION
    local N1_STATE_ROOT_HASH
    local NX_PROTOCOL_VERSION
    local NX_STATE_ROOT_HASH
    local RETRY_COUNT
    local NODE_COUNT

    log_step_upgrades 8 "asserting joined nodes are running upgrade"

    NODE_COUNT=$(get_count_of_nodes)
    # Status refresh.
    for NODE_ID in $(seq 1 $NODE_COUNT)
    do
        source "$NCTL"/sh/node/status.sh node="$NODE_ID"
    done

    # Assert all nodes are live.
    for NODE_ID in $(seq 1 $NODE_COUNT)
    do
        if [ $(get_node_is_up "$NODE_ID") == false ]; then
            log "node-$NODE_ID not up"
            log "NODE_COUNT_UP: $(get_count_of_up_nodes)"
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
    for NODE_ID in $(seq 2 "$(get_count_of_nodes)")
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
    for NODE_ID in $(seq 2 "$(get_count_of_nodes)")
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

    log "Waiting for all nodes to sync to genesis"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        await_node_historical_sync_to_genesis "$NODE_ID" "300"
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
