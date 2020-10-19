#!/usr/bin/env bash
#
# Interactively spins up a node within a network.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

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
node=${node:-1}

#######################################
# Main
#######################################

# Set rust log level.
export RUST_LOG=$loglevel

# Set path -> node config.
path_config=$NCTL/assets/net-$net/nodes/node-$node/config/node-config.toml

# Start node in validator mode.
$NCTL/assets/net-$net/bin/casper-node validator $path_config
