

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
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset gsh
unset global_state_hash
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        gsh) gsh=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
global_state_hash=${gsh:-""}
net=${net:-1}
node=${node:-1}

#######################################
# Main
#######################################

global_state_hash=2bad398b6acc726fd69cd251645bdd5c57cf47b5487ab52e554b159b705d6186
node_address=http://localhost:50101
account_hash=7959a737b66a3350289757a6dd9ec2217ee79a087a866add45a471bbc16b2e98

$CASPER_CLIENT query-state \
    -g $global_state_hash \
    -n $node_address \
    -k account-hash-$account_hash | python3 -m json.tool

exec_node_rpc $net $node "info_get_metrics"
