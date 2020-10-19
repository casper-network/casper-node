#!/usr/bin/env bash
#
# Renders node metrics to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Displays to stdout current node metrics.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifer.
#   Node ordinal identifer.
#   Metric name.
#######################################
function _view_metrics() {
    node_address=$(get_node_address $1 $2)
    log "network #$1 :: node #$2 :: $node_address :: metrics:"
    if [ $3 = "all" ]; then
        exec_node_rest_get $1 $2 "metrics"
    else
        exec_node_rest_get $1 $2 "metrics" | grep $3
    fi
}

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset metric
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        metric) metric=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
metric=${metric:-"all"}
net=${net:-1}
node=${node:-"all"}

#######################################
# Main
#######################################

if [ $node = "all" ]; then
    source $NCTL/assets/net-$net/vars
    for node_idx in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        echo $node_idx
        _view_metrics $net $node_idx $metric
        echo "------------------------------------------------------------------------------------------------------------------------------------"
        echo "------------------------------------------------------------------------------------------------------------------------------------"
    done
else
    _view_metrics $net $node $metric
fi
