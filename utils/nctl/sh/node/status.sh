#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/node/funcs_$NCTL_DAEMON_TYPE.sh

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

if [ $NODE_ID == "all" ]; then
    do_node_status_all $NET_ID $NCTL_NET_NODE_COUNT
else
    do_node_status $NET_ID $NODE_ID
fi
