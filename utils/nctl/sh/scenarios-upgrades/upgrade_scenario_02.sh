# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# Spins up a network, awaits for it to settle down and then performs a series of
# network upgrades.  At each step network behaviour is verified.

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
    local PATH_TO_STAGE
    local PROTOCOL_VERSION=""
    local UPGRADE_ID=0

    # Assert stage exists.
    PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")"
    if [ ! -d "$PATH_TO_STAGE" ]; then
        log "ERROR :: stage $STAGE_ID has not been built - cannot run scenario"
        exit 1
    fi

    # Iterate staged protocol versions:
    for FHANDLE in "$PATH_TO_STAGE/"*; do
        if [ -d "$FHANDLE" ]; then
            # ... spinup
            if [ "$PROTOCOL_VERSION" == "" ]; then
                _spinup "$STAGE_ID" "$PATH_TO_STAGE" "$(basename "$FHANDLE")"
            # ... upgrade
            else
                UPGRADE_ID=$((UPGRADE_ID + 1))
                _upgrade "$UPGRADE_ID" "$STAGE_ID" "$(basename "$FHANDLE")" "$PROTOCOL_VERSION"
            fi
            PROTOCOL_VERSION="$(basename "$FHANDLE")"
        fi
    done

    # Lastly sync new Nodes
    _sync_new_nodes
    _sync_new_nodes_test

    # Tidy up.
    log_break
    log "test successful - tidying up"
    source "$NCTL/sh/assets/teardown.sh"
    log_break
}

# Spinup: start network from pre-built stage.
function _spinup()
{
    local STAGE_ID=${1}
    local PATH_TO_STAGE=${2}
    local PROTOCOL_VERSION=${3}

    _spinup_step_01 "$STAGE_ID"
    _spinup_step_02
    _spinup_step_03
    _spinup_step_04
}

# Spinup: step 01: Start network from pre-built stage.
function _spinup_step_01()
{
    local STAGE_ID=${1}

    log_step_upgrades 1 "starting network from stage $STAGE_ID" "SPINUP"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
    source "$NCTL/sh/node/start.sh" node="all"
}

# Spinup: step 02: Await for genesis
function _spinup_step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion" "SPINUP"

    do_await_genesis_era_to_complete 'false'
}

# Spinup: step 03: Populate global state -> native + wasm transfers.
function _spinup_step_03()
{
    log_step_upgrades 3 "dispatching deploys to populate global state" "SPINUP"

    log "... 100 native transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_native.sh" \
        transfers=100 interval=0.0 verbose=false

    log "... 100 wasm transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_wasm.sh" \
        transfers=100 interval=0.0 verbose=false
}

# Spinup: step 04: Await era-id += 1.
function _spinup_step_04()
{
    log_step_upgrades 4 "awaiting next era" "SPINUP"

    await_n_eras 1
}

# Upgrade: Progress network to next upgrade from pre-built stage.
function _upgrade()
{
    local UPGRADE_ID=${1}
    local STAGE_ID=${2}
    local PROTOCOL_VERSION=${3}
    local PROTOCOL_VERSION_PREVIOUS=${4}

    _upgrade_step_01 "$UPGRADE_ID" "$STAGE_ID" "$PROTOCOL_VERSION" "$PROTOCOL_VERSION_PREVIOUS"
    _upgrade_step_02 "$UPGRADE_ID"
    _upgrade_step_03 "$UPGRADE_ID"
    _upgrade_step_04 "$UPGRADE_ID"
    _upgrade_step_05 "$UPGRADE_ID"
    _upgrade_step_06 "$UPGRADE_ID" "$PROTOCOL_VERSION_PREVIOUS"
}

# Upgrade: Upgrade network from stage.
function _upgrade_step_01()
{
    local UPGRADE_ID=${1}
    local STAGE_ID=${2}
    local PROTOCOL_VERSION=${3}
    local PROTOCOL_VERSION_PREVIOUS=${4}

    log_step_upgrades 1 "upgrading network from stage ($STAGE_ID) @ $PROTOCOL_VERSION_PREVIOUS -> $PROTOCOL_VERSION" "UPGRADE $UPGRADE_ID:"

    log "... setting upgrade assets"
    source "$NCTL/sh/assets/upgrade_from_stage.sh" stage="$STAGE_ID" verbose=false

    log "... awaiting upgrade"
    await_n_eras 4
}

# Upgrade: Populate global state -> native + wasm transfers.
function _upgrade_step_02()
{
    local UPGRADE_ID=${1}

    log_step_upgrades 2 "dispatching deploys to populate global state" "UPGRADE $UPGRADE_ID:"

    log "... ... 100 native transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_native.sh" \
        transfers=100 interval=0.0 verbose=false

    log "... ... 100 wasm transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_wasm.sh" \
        transfers=100 interval=0.0 verbose=false
}

# Upgrade: Await era-id += 1.
function _upgrade_step_03()
{
    log_step_upgrades 4 "awaiting next era" "UPGRADE $UPGRADE_ID:"

    await_n_eras 1
}

# Upgrade: Assert chain is live.
function _upgrade_step_04()
{
    local UPGRADE_ID=${1}

    log_step_upgrades 3 "asserting chain liveness" "UPGRADE $UPGRADE_ID:"

    if [ "$(get_count_of_up_nodes)" != "$(get_count_of_genesis_nodes)" ]; then
        log "ERROR :: protocol upgrade failure - >= 1 nodes have stopped"
        nctl-status
        exit 1
    fi
}

# Upgrade: Assert chain is progressing at all nodes.
function _upgrade_step_05()
{
    local UPGRADE_ID=${1}
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step_upgrades 5 "asserting chain progression" "UPGRADE $UPGRADE_ID:"

    HEIGHT_1=$(get_chain_height)
    await_n_blocks 2

    for NODE_ID in $(seq 1 "$(get_count_of_genesis_nodes)")
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -le "$HEIGHT_1" ]; then
            log "ERROR :: protocol upgrade failure - >= 1 nodes have stalled"
            exit 1
        fi
    done
}

function _upgrade_step_06()
{
    local UPGRADE_ID=${1}
    local PROTOCOL_VERSION_PREVIOUS=${2}
    local N1_PROTOCOL_VERSION
    local NX_PROTOCOL_VERSION

    log_step_upgrades 6 "asserting node upgrades" "UPGRADE $UPGRADE_ID:"

    # Assert node-1 protocol version incremented.
    N1_PROTOCOL_VERSION=$(get_node_protocol_version 1 | sed 's/\./\_/g')
    if [ "$N1_PROTOCOL_VERSION" == "$PROTOCOL_VERSION_PREVIOUS" ]; then
        log "ERROR :: protocol upgrade failure - >= protocol version did not increment"
        exit 1
    else
        log "Node 1 upgraded successfully: $PROTOCOL_VERSION_PREVIOUS -> $N1_PROTOCOL_VERSION"
    fi

    # Assert nodes are running same protocol version.
    for NODE_ID in $(seq 2 "$(get_count_of_genesis_nodes)")
    do
        NX_PROTOCOL_VERSION=$(get_node_protocol_version "$NODE_ID" | sed 's/\./\_/g')
        if [ "$NX_PROTOCOL_VERSION" != "$N1_PROTOCOL_VERSION" ]; then
            log "ERROR :: protocol upgrade failure - >= nodes are not all running same protocol version"
            exit 1
        else
            log "Node $NODE_ID upgraded successfully: $PROTOCOL_VERSION_PREVIOUS -> $NX_PROTOCOL_VERSION"
        fi
    done
}

function _sync_new_nodes()
{
    local NODE_ID
    local TRUSTED_HASH

    log_step_upgrades "1" "joining passive nodes" "SYNC"

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
    await_n_eras 2
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
    sleep 90
    await_n_eras 1
    await_n_blocks 1

}

function _sync_new_nodes_test()
{
    local NODE_ID
    local N1_BLOCK_HASH
    local N1_PROTOCOL_VERSION
    local N1_STATE_ROOT_HASH
    local NX_PROTOCOL_VERSION
    local NX_STATE_ROOT_HASH

    log_step_upgrades "2" "asserting joined nodes are running upgrade" "SYNC"

    # Assert all nodes are live.
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        if [ $(get_node_is_up "$NODE_ID") == false ]; then
            log "ERROR :: sync failure - >= 1 nodes not live"
            log "$(nctl-status)"
            exit 1
        fi
    done

    # Get node 1 proto version.
    N1_PROTOCOL_VERSION=$(get_node_protocol_version 1 | sed 's/\./\_/g')
    log "Node 1 running proto version: $N1_PROTOCOL_VERSION"

    # Assert nodes are running same protocol version.
    for NODE_ID in $(seq 2 "$(get_count_of_nodes)")
    do
        NX_PROTOCOL_VERSION=$(get_node_protocol_version "$NODE_ID" | sed 's/\./\_/g')
        if [ "$NX_PROTOCOL_VERSION" != "$N1_PROTOCOL_VERSION" ]; then
            log "ERROR :: protocol upgrade failure - >= node $NODE_ID is not running same protocol version"
            log "node 1 protocol version: $N1_PROTOCOL_VERSION"
            log "node $NODE_ID protocol version: $NX_PROTOCOL_VERSION"
            exit 1
        else
            log "Node $NODE_ID running correct proto version: $NX_PROTOCOL_VERSION"
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
            exit 1
        fi
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset _STAGE_ID

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
