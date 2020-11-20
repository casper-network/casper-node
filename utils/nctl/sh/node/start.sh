#!/bin/bash
#
# Starts up a node within a network.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset loglevel
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        loglevel) loglevel=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
loglevel=${loglevel:-$RUST_LOG}
loglevel=${loglevel:-debug}
net=${net:-1}
node=${node:-"all"}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Set rust log level.
export RUST_LOG=$loglevel

# Reset logs.
source $NCTL/sh/node/log_reset.sh net=$net node=$node

# Set daemon handler.
if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
    daemon_mgr=$NCTL/sh/daemons/supervisord/node_start.sh
fi

# Start node(s) by passing through to daemon specific handler.
# ... all nodes:
if [ $node = "all" ]; then
    log "network #$net: starting bootstraps ... "
    for idx in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        if [ $idx -le $NCTL_NET_BOOTSTRAP_COUNT ]; then
            log "network #$net: bootstrapping node $idx"
            source $daemon_mgr $net $idx
        fi
    done

    log "network #$net: starting non-bootstraps... "
    source $daemon_mgr $net all

    log "network #$net: bootstrapped & started"

# ... single nodes:
else
    log "network #$net: starting node ... "
    source $daemon_mgr $net $node
fi

# Display status.
sleep 1.0
source $NCTL/sh/node/status.sh net=$net
