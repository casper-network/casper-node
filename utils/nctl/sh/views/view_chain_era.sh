#!/usr/bin/env bash

source $NCTL/sh/utils.sh

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

if [ $NODE_ID = "all" ]; then
    for NODE_ID in $(seq 1 $(get_count_of_all_nodes $NET_ID))
    do
        log "chain era @ net-$NET_ID.node-$NODE_ID = $(get_chain_era $NET_ID $NODE_ID)"
    done
else
    log "chain era @ net-$NET_ID.node-$NODE_ID = $(get_chain_era $NET_ID $NODE_ID)"
fi
