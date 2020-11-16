#!/usr/bin/env bash
#
# Views ERC-20 contract details.
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
source $NCTL/sh/utils/misc.sh
source $NCTL/sh/contracts/erc20/utils.sh

# Import vars.path_net=$(get_path_to_net $net)

# Set contract owner account key.
contract_owner_account_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_FAUCET)

# Set contract hash.
contract_hash=$(get_erc20_contract_hash $net $node $contract_owner_account_key)

# Set token name.
token_name=$(get_erc20_contract_key_value $net $node $contract_hash "_name")

# Set token symbol.
token_symbol=$(get_erc20_contract_key_value $net $node $contract_hash "_symbol")

# Set token supply.
token_supply=$(get_erc20_contract_key_value $net $node $contract_hash "_totalSupply")

# Set token decimals.
token_decimals=$(get_erc20_contract_key_value $net $node $contract_hash "_decimals")

# Render details.
log "Contract Details -> ERC-20"
log "... on-chain name = ERC20"
log "... on-chain hash = "$contract_hash
log "... owner account = "$contract_owner_account_key
log "... token name = "$token_name
log "... token symbol = "$token_symbol
log "... token supply = "$token_supply
