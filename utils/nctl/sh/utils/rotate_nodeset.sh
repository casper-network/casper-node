#!/usr/bin/env bash

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/auction/funcs.sh
source $NCTL/sh/views/funcs.sh

NET_NODE_COUNT_ALL=$(get_count_of_all_nodes)
NET_NODE_COUNT_GENESIS=$(get_count_of_genesis_nodes)
NODE_ID_FOR_DISPATCH=1
ROTATE_STEP=0

function log_step()
{
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $ROTATE_STEP: $COMMENT"
    ROTATE_STEP=$(($ROTATE_STEP + 1))
}

log "------------------------------------------------------------"
log "Network nodeset rotation begins"
log "------------------------------------------------------------"


log_step "state root hash at nodes:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_ALL)
do
    render_chain_state_root_hash $NODE_ID
done


log_step "awaiting genesis era to complete"
while [ $(get_chain_era) -lt 1 ];
do
    sleep 1.0
done


log_step "submitting POS auction bids:"
for NODE_ID in $(seq $(($NET_NODE_COUNT_GENESIS + 1)) $NET_NODE_COUNT_ALL)
do
    BID_AMOUNT=$(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))
    BID_DELEGATION_RATE=125

    source $NCTL/sh/contracts/auction/do_bid.sh \
        bidder=$NODE_ID \
        amount=$BID_AMOUNT \
        rate=$BID_DELEGATION_RATE \
        quiet="TRUE"

    log "node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"
done


log_step "awaiting 4 eras"
await_n_eras 4 true


log_step "starting non-genesis nodes:"
for NODE_ID in $(seq $(($NET_NODE_COUNT_GENESIS + 1)) $NET_NODE_COUNT_ALL)
do
    source $NCTL/sh/node/start.sh node=$NODE_ID
    log "node-$NODE_ID started"
done


log_step "awaiting 10 seconds for non-genesis nodes to spin-up & join network"
sleep 10.0


log_step "submitting auction withdrawals:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_GENESIS)
do
    WITHDRAWAL_AMOUNT=$(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))

    source $NCTL/sh/contracts/auction/do_bid_withdraw.sh \
        bidder=$NODE_ID \
        amount=$WITHDRAWAL_AMOUNT \
        quiet="TRUE"

    log "node-$NODE_ID auction bid withdrawn -> $WITHDRAWAL_AMOUNT CSPR"
done


log_step "awaiting 15 eras prior to bringing down genesis nodes"
await_n_eras 15


log_step "stopping genesis nodes"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_GENESIS)
do
    source $NCTL/sh/node/stop.sh node=$NODE_ID
    sleep 1.0
    log "node-$NODE_ID stopped"
done


log_step "state root hash at nodes:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_ALL)
do
    render_chain_state_root_hash $NODE_ID
done


log "------------------------------------------------------------"
log "Network nodeset rotation complete"
log "------------------------------------------------------------"
