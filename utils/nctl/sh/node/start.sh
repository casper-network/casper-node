#!/usr/bin/env bash

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
export RUST_LOG=$LOG_LEVEL
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh
source $NCTL/sh/node/funcs_$NCTL_DAEMON_TYPE.sh

if [ $NODE_ID == "all" ]; then
    do_node_start_all \
        $NET_ID \
        $(get_count_of_genesis_nodes $NET_ID) \
        $(get_count_of_bootstrap_nodes $NET_ID)
else
    log "starting node :: net-$NET_ID.node-$NODE_ID"
    do_node_start $NET_ID $NODE_ID
fi

sleep 1.0
source $NCTL/sh/node/status.sh net=$NET_ID node=$NODE_ID
