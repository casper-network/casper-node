# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# 1. Start v1 running at current mainnet commit.
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
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}

    log_step 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await era-id >= 1.
function _step_02()
{
    log_step 2 "awaiting genesis era completion"

    sleep 60.0
    await_until_era_n 1
}

# Step 03: Populate global state -> native + wasm transfers.
function _step_03()
{
    log_step 3 "dispatching deploys to populate global state"

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
    log_step 4 "awaiting next era"

    await_n_eras 1
}

# Step 05: Upgrade network from stage.
function _step_05()
{
    local STAGE_ID=${1}

    log_step 5 "upgrading network from stage ($STAGE_ID)"

    log "... setting upgrade assets"
    source "$NCTL/sh/assets/upgrade_from_stage.sh" stage="$STAGE_ID" verbose=false

    log "... awaiting 2 eras + 1 block"
    await_n_eras 2
    await_n_blocks 1
}

# Step 06: Assert chain is progressing at all nodes.
function _step_06()
{
    local N1_PROTOCOL_VERSION_INITIAL=${1}
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step 6 "asserting node upgrades"

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

# Step 07: Join passive nodes.
function _step_07()
{
    local NODE_ID
    local TRUSTED_HASH

    log_step 7 "joining passive nodes"

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

    log "... awaiting auction bid acceptance (3 eras + 1 block)"
    await_n_eras 3
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
    sleep 60
    await_n_eras 1
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

    log_step 8 "asserting joined nodes are running upgrade"

    # Assert all nodes are live.
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
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
        NX_STATE_ROOT_HASH=$(get_state_root_hash "$NODE_ID" "$N1_BLOCK_HASH")
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

# Step 09: Terminate.
function _step_09()
{
    log_step 7 "test successful - tidying up"

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
