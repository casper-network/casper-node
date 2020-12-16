#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $(get_path_to_net_vars $NET_ID)
source $NCTL/sh/node/ctl_$NCTL_DAEMON_TYPE.sh

unset LOG_LEVEL
unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        loglevel) LOG_LEVEL=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-"all"}

export RUST_LOG=$LOG_LEVEL

if [ $NODE_ID == "all" ]; then
    do_node_start_all $NET_ID $NCTL_NET_NODE_COUNT $NCTL_NET_BOOTSTRAP_COUNT
else
    log "starting node :: net-$NET_ID.node-$NODE_ID"
    do_node_start $NET_ID $NODE_ID
fi

sleep 1.0
source $NCTL/sh/node/status.sh net=$NET_ID node=$NODE_ID
