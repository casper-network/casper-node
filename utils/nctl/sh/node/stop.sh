#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/node/ctl_$NCTL_DAEMON_TYPE.sh

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

NODE_COUNT=$()
BOOTSTRAP_COUNT=$()

if [ $NODE_ID == "all" ]; then
    do_node_stop_all $NET_ID $NCTL_NET_NODE_COUNT $NCTL_NET_BOOTSTRAP_COUNT
else
    log "net-$NET_ID:node-$NODE_ID: stopping node ... "
    do_node_stop $NET_ID $NODE_ID
fi

sleep 1.0
source $NCTL/sh/node/status.sh net=$NET_ID node=$NODE_ID
