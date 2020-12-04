#!/usr/bin/env bash
#
# Renders node status to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

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

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $net)

# Render node status.
if [ $node = "all" ]; then
    for IDX in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        echo "------------------------------------------------------------------------------------------------------------------------------------"
        render_node_status $net $IDX
    done
    echo "------------------------------------------------------------------------------------------------------------------------------------"
else
    render_node_status $net $node
fi
