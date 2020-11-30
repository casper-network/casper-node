#!/usr/bin/env bash
#
# Fund user accounts from faucet.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier (optional).
#   Node ordinal identifier (optional).
#   Gas price (optional).
#   Gas payment (optional).

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
source $NCTL/sh/utils.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Inform.
log "funding user accounts from faucet:"
log "... network=$net"
log "... node=$node"
log "... transfer amount=$NCTL_INITIAL_BALANCE_USER"
log "... dispatched deploys:"

# Set faucet secret key.
path_to_faucet_secret_key=$(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_FAUCET)

for idx in $(seq 1 $NCTL_NET_USER_COUNT)
do
    # Set user account key.
    user_account_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $idx)

    # Dispatch deploy.
    deploy_hash=$(
        $(get_path_to_client $net) transfer \
            --chain-name $(get_chain_name $net) \
            --gas-price $gas \
            --node-address $(get_node_address_rpc $net $node) \
            --payment-amount $payment \
            --secret-key $path_to_faucet_secret_key \
            --ttl "1day" \
            --amount $NCTL_INITIAL_BALANCE_USER \
            --target-account $user_account_key \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
    log "... ... user-"$idx" -> "$deploy_hash
done
