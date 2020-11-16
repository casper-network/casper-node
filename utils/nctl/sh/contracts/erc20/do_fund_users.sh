#!/usr/bin/env bash
#
# Transfers ERC-20 token balances from faucet account to users.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier (optional).
#   Node ordinal identifier (optional).
#   Gas price (optional).
#   Gas payment (optional).
#   Amount to be transferred (optional).
#   Node ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset amount
unset gas
unset gas_payment
unset gas_price
unset net
unset node

# Destructure named args.
for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        # ... standard args
        gas) gas_price=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) gas_payment=${VALUE} ;;
        # ... custom args
        amount) amount=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:-1000000000}
gas_payment=${gas_payment:-1000000000}
gas_price=${gas_price:-$NCTL_DEFAULT_GAS_PRICE}
net=${net:-1}
node=${node:-1}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Import vars.
source $(get_path_to_net_vars $net)

# Set amount - target contract uses U256 therefore need to factor to U512.
amount=$(($amount * 2))

# Set contract owner secret key.
contract_owner_secret_key=$(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_FAUCET)

# Set deploy dispatch node address. 
node_address=$(get_node_address_rpc $net $node)

# For each user, dispatch a deploy:
for idx in $(seq 1 $NCTL_NET_USER_COUNT)
do
    # Set user account key.
    user_account_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $idx)

    # Set user account hash.
    user_account_hash=$(get_account_hash $user_account_key)

    # Set deploy hash.
    deploy_hash=$(
        $(get_path_to_client $net) put-deploy \
            --chain-name casper-net-$net \
            --gas-price $gas_price \
            --node-address $node_address \
            --payment-amount $gas_payment \
            --secret-key $contract_owner_secret_key \
            --ttl "1day" \
            --session-name "ERC20" \
            --session-entry-point "transfer" \
            --session-arg "recipient:account_hash='account-hash-$user_account_hash'" \
            --session-arg "amount:U256='$amount'" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    )

    # Inform.
    log "funding user $idx deploy hash = "$deploy_hash
done
