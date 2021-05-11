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

_STAGE_ID="${_STAGE_ID:-1}"

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
    _step_03
    _step_04
    _step_05 "$STAGE_ID"
    _step_06
    _step_07
    _step_08
    _step_09
    _step_10
}

# Step 01: Start network from pre-built stage.
function _step_01() 
{
    local STAGE_ID=${1}

    log_step 1 "starting network from stage $STAGE_ID"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await era-id >= 1.
function _step_02() 
{
    log_step 2 "awaiting genesis era completion"

    await_until_era_n 1
}

# Step 03: Populate global state -> native + wasm transfers.
function _step_03() 
{
    log_step 3 "dispatching deploys to populate global state"

    log "... ... 100 native transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_native.sh" \
        transfers=100 interval=0.0 verbose=false

    log "... ... 100 wasm transfers"
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

    source "$NCTL/sh/assets/upgrade_from_stage.sh" stage="$STAGE_ID"
    sleep 10.0
}

# Step 06: Assert chain is progressing at all nodes.
function _step_06() 
{
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
}

# Step 07: Join passive nodes.
function _step_07() 
{
    local NODE_ID
    local TRUSTED_HASH

    log_step 7 "joining passive nodes"

    log "... ... submitting auction bids"
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

    log "... ... awaiting auction bid acceptance (3 eras + 1 block)"
    await_n_eras 3
    await_n_blocks 1

    log "... ... starting nodes"
    TRUSTED_HASH="$(get_chain_latest_block_hash)"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        if [ $(get_node_is_up "$NODE_ID") == false ]; then
            do_node_start "$NODE_ID" "$TRUSTED_HASH"
        fi    
    done    
}

# Step 09: Await era-id += 2.
function _step_08() 
{
    log_step 8 "awaiting era rotation"

    await_n_eras 2
}

# Step 09: Assert passive joined & are running upgrade.
function _step_09() 
{
    log_step 9 "asserting joined nodes are running upgrade"

    # Assert all nodes are live.
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        if [ $(get_node_is_up "$NODE_ID") == false ]; then
            log "ERROR :: protocol upgrade failure - >= 1 nodes not live"
            exit 1            
        fi    
    done
}

# Step 10: Terminate.
function _step_10() 
{
    log_step 10 "test successful - tidying up"

    source "$NCTL/sh/assets/teardown.sh"
}

# Call entry point.
_main "$_STAGE_ID"
