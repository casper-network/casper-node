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
# Main
#######################################

log "------------------------------------------------------------"
log "Network nodeset rotation begins"

# Set network node count - we will be doubling the size & then contracting.
NET_NODE_COUNT=$(get_count_of_genesis_nodes $NET_ID)

# Dispatch deploys to node 1.
DEPLOY_DISPATCH_NODE_ID=1


log "------------------------------------------------------------"
log "STEP 01: state root hash at active nodes:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT)
do
    render_chain_state_root_hash $NET_ID $NODE_ID
done


log "------------------------------------------------------------"
log "STEP 02: starting non-genesis nodes:"
for IDX in $(seq 1 $NET_NODE_COUNT)
do
    NODE_ID=$(($NET_NODE_COUNT + $IDX))
    do_node_start $NET_ID $NODE_ID
    log "net-$NET_ID:node-$NODE_ID started"
done


log "------------------------------------------------------------"
log "STEP 03: awaiting 60 seconds for non-genesis nodes to fully bind"
sleep 60.0


log "------------------------------------------------------------"
log "STEP 04: state root hash at active nodes:"
for NODE_ID in $(seq 1 $(($NET_NODE_COUNT * 2)))
do
    render_chain_state_root_hash $NET_ID $NODE_ID
done


log "------------------------------------------------------------"
log "STEP 05: submitting auction bids:"
for IDX in $(seq 1 $NET_NODE_COUNT)
do
    NODE_ID=$(($NET_NODE_COUNT + $IDX))
    BID_AMOUNT=$(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))
    DELEGATION_RATE=125

    do_auction_bid_submit \
        $NET_ID \
        $DEPLOY_DISPATCH_NODE_ID \
        $NODE_ID \
        $BID_AMOUNT \
        $DELEGATION_RATE \
        $NCTL_DEFAULT_GAS_PRICE \
        $NCTL_DEFAULT_GAS_PAYMENT \
        "TRUE"

    log "net-$NET_ID:node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"
done
sleep 1.0


log "------------------------------------------------------------"
log "STEP 06: submitting auction withdrawals:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT)
do
    WITHDRAWAL_AMOUNT=$(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))
    do_auction_bid_withdraw \
        $NET_ID \
        $DEPLOY_DISPATCH_NODE_ID \
        $NODE_ID \
        $WITHDRAWAL_AMOUNT \
        $NCTL_DEFAULT_GAS_PRICE \
        $NCTL_DEFAULT_GAS_PAYMENT \
        "TRUE"
    log "net-$NET_ID:node-$NODE_ID auction bid withdrawn -> $WITHDRAWAL_AMOUNT CSPR"
done

log "------------------------------------------------------------"
log "STEP 07: awaiting 180 seconds for genesis nodes to be ejected from active validator set"
# sleep 180.0


# log "------------------------------------------------------------"
# log "STEP 08: stopping genesis nodes"
# for NODE_ID in $(seq 1 $NET_NODE_COUNT)
# do
#     do_node_stop $NET_ID $NODE_ID
#     sleep 1.0
#     log "net-$NET_ID:node-$NODE_ID stopped"
# done

log "------------------------------------------------------------"
log "STEP 09: state root hash at active nodes:"
for NODE_ID in $(seq 1 $NET_NODE_COUNT)
do
    render_chain_state_root_hash $NET_ID $NODE_ID
done
for IDX in $(seq 1 $NET_NODE_COUNT)
do
    NODE_ID=$(($NET_NODE_COUNT + $IDX))
    render_chain_state_root_hash $NET_ID $NODE_ID
done

log "------------------------------------------------------------"
log "Network nodeset rotation complete"
log "------------------------------------------------------------"
