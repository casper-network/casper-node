#!/usr/bin/env bash
#
# Views ERC-20 token balances.
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

# Destructure named args.
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
node=${node:-1}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/erc20/utils.sh

# Import net vars.
path_net=$(get_path_to_net $net)

# Set contract owner account key.
contract_owner_account_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_FAUCET)

# Set contract owner account hash.
contract_owner_account_hash=$(get_account_hash $contract_owner_account_key)

# Set contract hash.
contract_hash=$(get_erc20_contract_hash $net $node $contract_owner_account_key)

# Set contract owner account balance.
contract_owner_account_balance_key="_balances_"$contract_owner_account_hash
contract_owner_account_balance=$(get_erc20_contract_key_value $net $node $contract_hash $contract_owner_account_balance_key)

# Render contract owner account balance.
log "ERC-20 $token_symbol Account Balances:"
log "... contract owner = "$contract_owner_account_balance

# Render user account balances.
for IDX in $(seq 1 $NCTL_NET_USER_COUNT)
do
    # Set user account key.
    user_account_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $IDX)

    # Set user account hash.
    user_account_hash=$(get_account_hash $user_account_key)

    # Set user account balance.
    user_account_balance_key="_balances_"$user_account_hash
    user_account_balance=$(get_erc20_contract_key_value $net $node $contract_hash $user_account_balance_key)

    # Render user account balance.
    log "... user $IDX = "$(($user_account_balance * 2))
done
