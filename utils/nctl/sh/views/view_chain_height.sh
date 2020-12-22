#!/usr/bin/env bash

unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh

if [ $NODE_ID = "all" ]; then
    for NODE_ID in $(seq 1 $(get_count_of_all_nodes $NET_ID))
    do
        log "chain height @ net-$NET_ID.node-$NODE_ID = $(get_chain_height $NET_ID $NODE_ID)"
    done
else
    log "chain height @ net-$NET_ID.node-$NODE_ID = $(get_chain_height $NET_ID $NODE_ID)"
fi
