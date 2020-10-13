#!/usr/bin/env bash
#
# Emits to stdout details regarding a node's storage.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Displays to stdout current node storage stats.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifer.
#   Node ordinal identifer.
#######################################
function _view_storage() {
    declare os_type="$(get_os)"
    declare path_node_storage=$NCTL/assets/net-$1/nodes/node-$2/storage/*.db
    log "network #$1 :: node #$2 :: storage stats:"
    if [[ $os_type == $_OS_LINUX* ]]; then
        ll $path_node_storage
    elif [[ $os_type == $_OS_MACOSX ]]; then
        ls -lG $path_node_storage
    fi
}

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-"all"}

#######################################
# Main
#######################################

if [ $node = "all" ]; then
    source $NCTL/assets/net-$net/vars
    for node_idx in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        _view_storage $net $node_idx
        echo "------------------------------------------------------------------------------------------------------------------------------------"
        echo "------------------------------------------------------------------------------------------------------------------------------------"
    done
else
    _view_storage $net $node
fi
