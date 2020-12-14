#!/usr/bin/env bash
#
# Displays node status.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier (optional).

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
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

# Set defaults.
NET_ID=${NET_ID:-1}

#######################################
# Imports
#######################################

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/auction/funcs.sh
source $NCTL/sh/node/ctl_$NCTL_DAEMON_TYPE.sh

#######################################
# FUNCS
#######################################

function await_n_eras()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local ERA_OFFSET=${3}

    local ERA_ID=$(get_chain_era $NET_ID $NODE_ID)
    local ERA_ID_OFFSET=$(($ERA_ID + $ERA_OFFSET))

    while [ $ERA_ID -lt $ERA_ID_OFFSET ];
    do
        sleep 10.0
        ERA_ID=$(get_chain_era $NET_ID $NODE_ID)
    done
}

#######################################
# Main
#######################################

log "------------------------------------------------------------"
log "Network nodeset rotation begins"

# Set network node count - we will be doubling the size & then contracting.
NET_NODE_COUNT_ALL=$(get_count_of_all_nodes $NET_ID)
NET_NODE_COUNT_GENESIS=$(get_count_of_genesis_nodes $NET_ID)

# Dispatch deploys to node 1.
NODE_ID_FOR_DISPATCH=1


log "------------------------------------------------------------"
log "STEP 01: state root hash at nodes:"

for NODE_ID in $(seq 1 $NET_NODE_COUNT_ALL)
do
    render_chain_state_root_hash $NET_ID $NODE_ID
done


log "------------------------------------------------------------"
log "STEP 02: awaiting genesis era to complete"

while [ $(get_chain_era $NET_ID $NODE_ID_FOR_DISPATCH) -lt 1 ];
do
    sleep 1.0
done


log "------------------------------------------------------------"
log "STEP 03: submitting POS auction bids:"
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


log "------------------------------------------------------------"
log "STEP 04: awaiting 4 eras"
await_n_eras $NET_ID $NODE_ID_FOR_DISPATCH 4


log "------------------------------------------------------------"
log "STEP 05: starting non-genesis nodes:"
for NODE_ID in $(seq $(($NET_NODE_COUNT_GENESIS + 1)) $NET_NODE_COUNT_ALL)
do
    do_node_start $NET_ID $NODE_ID
    log "net-$NET_ID:node-$NODE_ID started"
done


log "------------------------------------------------------------"
log "STEP 06: awaiting 10 seconds for non-genesis nodes to spin-up & join network"
sleep 10.0


log "------------------------------------------------------------"
log "STEP 06: submitting auction withdrawals:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT)
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


log "------------------------------------------------------------"
log "STEP 07: awaiting 15 eras prior to bringing down genesis nodes"
await_n_eras $NET_ID $NODE_ID_FOR_DISPATCH 15


log "------------------------------------------------------------"
log "STEP 08: stopping genesis nodes"
for NODE_ID in $(seq 1 $NET_NODE_COUNT)
do
    do_node_stop $NET_ID $NODE_ID
    sleep 1.0
    log "net-$NET_ID:node-$NODE_ID stopped"
done

log "------------------------------------------------------------"
log "STEP 09: state root hash at nodes:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT_ALL)
do
    render_chain_state_root_hash $NET_ID $NODE_ID
done

log "------------------------------------------------------------"
log "Network nodeset rotation complete"
log "------------------------------------------------------------"
