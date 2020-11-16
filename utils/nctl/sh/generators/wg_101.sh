#!/usr/bin/env bash
#
# Dispatches wasmless transfers to a test net.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   User ordinal identifier.
#   Transfer dispatch count.
#   Transfer dispatch interval.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset gas
unset net
unset node
unset payment

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        gas) gas=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) payment=${VALUE} ;;
        *)
    esac
done

# Set defaults.
gas=${gas:-$NCTL_DEFAULT_GAS_PRICE}
net=${net:-1}
node=${node:-1}
payment=${payment:-$NCTL_DEFAULT_GAS_PAYMENT}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Inform.
log "funding user accounts from faucet:"
log "... network=$net"
log "... node=$node"
log "... transfer amount=$NCTL_INITIAL_BALANCE_USER"
log "... dispatched deploys:"

for idx in $(seq 1 $NCTL_NET_USER_COUNT)
do
    deploy_hash=$(
        $(get_path_to_client $net) transfer \
            --chain-name $(get_chain_name $net) \
            --gas-price $gas \
            --node-address $(get_node_address_rpc $net $node) \
            --payment-amount $payment \
            --secret-key $(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_FAUCET) \
            --ttl "1day" \
            --amount $NCTL_INITIAL_BALANCE_USER \
            --target-account $(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $idx) \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
    log "... ... user-"$idx" -> "$deploy_hash
done
