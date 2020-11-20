#!/usr/bin/env bash
#
# Renders a state root hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset block
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        block) block=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-"all"}
block=${block:-""}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Render state root hash.
if [ $node = "all" ]; then
    for idx in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        render_chain_state_root_hash $net $idx $block
    done
else
    render_chain_state_root_hash $net $node $block
fi
