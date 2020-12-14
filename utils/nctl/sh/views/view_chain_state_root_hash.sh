#!/usr/bin/env bash

source $NCTL/sh/utils.sh

unset BLOCK_HASH
unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        block) BLOCK_HASH=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-"all"}
BLOCK_HASH=${BLOCK_HASH:-""}

if [ $NODE_ID = "all" ]; then
    for NODE_ID in $(seq 1 $(get_count_of_all_nodes $NET_ID))
    do
        render_chain_state_root_hash $NET_ID $NODE_ID $BLOCK_HASH
    done
else
    render_chain_state_root_hash $NET_ID $NODE_ID $BLOCK_HASH
fi
