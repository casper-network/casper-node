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

source $(get_path_to_net_vars $NET_ID)
if [ $NODE_ID = "all" ]; then
    for NODE_ID in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        rm $(get_path_to_node $NET_ID $NODE_ID)/logs/*.log > /dev/null 2>&1
    done
else
    rm $(get_path_to_node $NET_ID $NODE_ID)/logs/*.log > /dev/null 2>&1
fi
