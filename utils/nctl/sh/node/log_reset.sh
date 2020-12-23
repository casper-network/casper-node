#!/usr/bin/env bash

source $NCTL/sh/utils.sh

unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $(get_path_to_net_vars)
if [ $NODE_ID = "all" ]; then
    for NODE_ID in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        rm $(get_path_to_node $NODE_ID)/logs/*.log > /dev/null 2>&1
    done
else
    rm $(get_path_to_node $NODE_ID)/logs/*.log > /dev/null 2>&1
fi
