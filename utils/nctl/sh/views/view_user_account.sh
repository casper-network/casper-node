#!/usr/bin/env bash
#
# Renders a network faucet account key.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net
unset node
unset user

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-1}
user=${user:-"all"}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Render account(s).
if [ $user = "all" ]; then
    for idx in $(seq 1 $NCTL_NET_USER_COUNT)
    do
        echo "------------------------------------------------------------------------------------------------------------------------------------"
        log "network #$net :: user #$idx :: on-chain account details:"
        render_account $net $node $NCTL_ACCOUNT_TYPE_USER $idx
    done
    echo "------------------------------------------------------------------------------------------------------------------------------------"
else
    log "network #$net :: user #$user :: on-chain account details:"
    render_account $net $node $NCTL_ACCOUNT_TYPE_USER $user
fi
