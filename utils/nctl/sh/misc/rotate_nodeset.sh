#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

function main()
{
    log "------------------------------------------------------------"
    log "Network nodeset rotation begins"
    log "------------------------------------------------------------"
    
    do_display_root_state_hashes

    do_await_genesis_era_to_complete

    do_submit_auction_bids
    do_start_newly_bonded_nodes

    do_submit_auction_withdrawals
    do_await_unbonding_eras_to_complete

    do_stop_genesis_nodes
    
    do_display_root_state_hashes

    log "------------------------------------------------------------"
    log "Network nodeset rotation complete"
    log "------------------------------------------------------------"
}

function log_step()
{
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    STEP=$((STEP + 1))
}

function do_await_pre_bonding_eras_to_complete()
{
    log_step "awaiting 4 eras"
    await_n_eras 4 true
}

function do_await_unbonding_eras_to_complete()
{
    log_step "awaiting 15 eras prior to bringing down genesis nodes"
    await_n_eras 15
}

function do_display_root_state_hashes()
{
    log_step "state root hash at nodes:"
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        render_chain_state_root_hash "$NODE_ID"
    done
}

function do_start_newly_bonded_nodes()
{
    log_step "starting non-genesis nodes:"
    for NODE_ID in $(seq $(($(get_count_of_genesis_nodes) + 1)) "$(get_count_of_nodes)")
    do
        source "$NCTL"/sh/node/start.sh node="$NODE_ID"
        log "node-$NODE_ID started"
    done

    log_step "awaiting 10 seconds for non-genesis nodes to spin-up & join network"
    sleep 10.0    
}

function do_stop_genesis_nodes()
{
    log_step "stopping genesis nodes"
    for NODE_ID in $(seq 1 "$(get_count_of_genesis_nodes)")
    do
        source "$NCTL"/sh/node/stop.sh node="$NODE_ID"
        sleep 1.0
        log "node-$NODE_ID stopped"
    done  
}

function do_submit_auction_bids()
{
    log_step "submitting POS auction bids:"
    for NODE_ID in $(seq $(($(get_count_of_genesis_nodes) + 1)) "$(get_count_of_nodes)")
    do
        log "----- ----- ----- ----- ----- -----"
        BID_AMOUNT=$(get_node_staking_weight "$NODE_ID")
        BID_DELEGATION_RATE=2

        source "$NCTL"/sh/contracts-auction/do_bid.sh \
            node="$NODE_ID" \
            amount="$BID_AMOUNT" \
            rate="$BID_DELEGATION_RATE" \
            quiet="TRUE"

        log "node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"
    done

    log_step "awaiting 10 seconds for auction bid deploys to finalise"
    sleep 10.0
}

function do_submit_auction_withdrawals()
{
    local WITHDRAWAL_AMOUNT

    log_step "submitting auction withdrawals:"
    for NODE_ID in $(seq 1 "$(get_count_of_genesis_nodes)")
    do
        WITHDRAWAL_AMOUNT=$(get_node_staking_weight "$NODE_ID")

        source "$NCTL"/sh/contracts-auction/do_bid_withdraw.sh \
            node="$NODE_ID" \
            amount="$WITHDRAWAL_AMOUNT" \
            quiet="TRUE"

        log "node-$NODE_ID auction bid withdrawn -> $WITHDRAWAL_AMOUNT CSPR"
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

STEP=0

main
