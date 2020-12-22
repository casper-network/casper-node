#!/usr/bin/env bash

unset NET_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        *)
    esac
done

NET_ID=${NET_ID:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/auction/funcs.sh
source $NCTL/sh/views/funcs.sh

NET_NODE_COUNT_ALL=$(get_count_of_all_nodes $NET_ID)
NET_NODE_COUNT_GENESIS=$(get_count_of_genesis_nodes $NET_ID)
NODE_ID_FOR_DISPATCH=1
ROTATE_STEP=0

function log_step() {
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
    render_chain_state_root_hash $NET_ID $NODE_ID
done


log_step "awaiting genesis era to complete"
while [ $(get_chain_era $NET_ID $NODE_ID_FOR_DISPATCH) -lt 1 ];
do
    sleep 1.0
done


log_step "submitting POS auction bids:"
for NODE_ID in $(seq $(($NET_NODE_COUNT_GENESIS + 1)) $NET_NODE_COUNT_ALL)
do
    BID_AMOUNT=$(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))
    DELEGATION_RATE=125

    do_auction_bid_submit \
        $NET_ID \
        $NODE_ID_FOR_DISPATCH \
        $NODE_ID \
        $BID_AMOUNT \
        $DELEGATION_RATE \
        $NCTL_DEFAULT_GAS_PRICE \
        $NCTL_DEFAULT_GAS_PAYMENT \
        "TRUE"

    log "net-$NET_ID:node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"
done


log_step "awaiting 4 eras"
await_n_eras $NET_ID $NODE_ID_FOR_DISPATCH 4 true


log_step "starting non-genesis nodes:"
for NODE_ID in $(seq $(($NET_NODE_COUNT_GENESIS + 1)) $NET_NODE_COUNT_ALL)
do
    source $NCTL/sh/node/start.sh net=$NET_ID node=$NET_ID
    log "net-$NET_ID:node-$NODE_ID started"
done


log_step "awaiting 10 seconds for non-genesis nodes to spin-up & join network"
sleep 10.0


log_step "submitting auction withdrawals:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_GENESIS)
do
    WITHDRAWAL_AMOUNT=$(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))

    do_auction_bid_withdraw \
        $NET_ID \
        $NODE_ID_FOR_DISPATCH \
        $NODE_ID \
        $WITHDRAWAL_AMOUNT \
        $NCTL_DEFAULT_GAS_PRICE \
        $NCTL_DEFAULT_GAS_PAYMENT \
        "TRUE"

    log "net-$NET_ID:node-$NODE_ID auction bid withdrawn -> $WITHDRAWAL_AMOUNT CSPR"
done


log_step "awaiting 15 eras prior to bringing down genesis nodes"
await_n_eras $NET_ID $NODE_ID_FOR_DISPATCH 15


log_step "stopping genesis nodes"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_GENESIS)
do
    source $NCTL/sh/node/stop.sh net=$NET_ID node=$NET_ID
    sleep 1.0
    log "net-$NET_ID:node-$NODE_ID stopped"
done


log_step "state root hash at nodes:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_ALL)
do
    render_chain_state_root_hash $NET_ID $NODE_ID
done


log "------------------------------------------------------------"
log "Network nodeset rotation complete"
log "------------------------------------------------------------"
