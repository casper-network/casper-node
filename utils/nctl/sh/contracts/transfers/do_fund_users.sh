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
unset GAS
unset NET_ID
unset NODE_ID
unset PAYMENT

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        gas) GAS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        payment) PAYMENT=${VALUE} ;;
        *)
    esac
done

#######################################
# Defaults
#######################################

GAS=${GAS:-$NCTL_DEFAULT_GAS_PRICE}
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}
PAYMENT=${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $NET_ID)

# Inform.
log "funding user accounts from faucet:"
log "... network=$NET_ID"
log "... node=$NODE_ID"
log "... transfer amount=$NCTL_INITIAL_BALANCE_USER"
log "... dispatched deploys:"

# Set faucet secret key.
PATH_TO_FAUCET_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_FAUCET)

for IDX in $(seq 1 $NCTL_NET_USER_COUNT)
do
    # Set user account key.
    USER_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $IDX)

    # Dispatch deploy.
    deploy_hash=$(
        $(get_path_to_client $NET_ID) transfer \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $(get_node_address_rpc $NET_ID $NODE_ID) \
            --payment-amount $PAYMENT \
            --secret-key $PATH_TO_FAUCET_SECRET_KEY \
            --ttl "1day" \
            --amount $NCTL_INITIAL_BALANCE_USER \
            --target-account $USER_ACCOUNT_KEY \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
    log "... ... user-"$IDX" -> "$deploy_hash
done
