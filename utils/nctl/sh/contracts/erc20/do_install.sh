#!/usr/bin/env bash
#
# Installs ERC-20 contract in readiness for use.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset gas
unset gas_payment
unset gas_price
unset net
unset node
unset name
unset supply
unset symbol

# Destructure named args.
for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        gas) gas_price=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        name) token_name=${VALUE} ;;
        payment) gas_payment=${VALUE} ;;
        supply) token_supply=${VALUE} ;;
        symbol) token_symbol=${VALUE} ;;
        *)
    esac
done

# Set defaults.
gas_payment=${gas_payment:-70000000000}
gas_price=${gas_price:-$NCTL_DEFAULT_GAS_PRICE}
net=${net:-1}
node=${node:-1}
token_name=${token_name:-"Acme Token"}
token_symbol=${token_symbol:-"ACME"}
token_supply=${token_supply:-1000000000000000000000000000000000}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $net)

# Set contract path.
contract_path=$(get_path_to_contract $net "erc20.wasm")
if [ ! -f $contract_path ]; then
    echo "ERROR: The erc20.wasm binary file cannot be found.  Please compile it and move it to the following directory: "$(get_path_to_net $net)
    return
fi

# Set contract owner secret key.
contract_owner_secret_key=$(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_FAUCET)

# Set contract owner account key.
contract_owner_account_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_FAUCET)

# Set contract owner account hash.
contract_owner_account_hash=$(get_account_hash $contract_owner_account_key)

# Set deploy dispatch node address. 
node_address=$(get_node_address_rpc $net $node)

# Set deploy hash.
deploy_hash=$(
    $(get_path_to_client $net) put-deploy \
        --chain-name casper-net-$net \
        --gas-price $gas_price \
        --node-address $node_address \
        --payment-amount $gas_payment \
        --secret-key $contract_owner_secret_key \
        --session-path $contract_path \
        --session-arg "tokenName:string='$token_name'" \
        --session-arg "tokenSymbol:string='$token_symbol'" \
        --session-arg "tokenTotalSupply:u512='$token_supply'" \
        --ttl "1day" \
        | jq '.result.deploy_hash' \
        | sed -e 's/^"//' -e 's/"$//'
    )

# Inform.
log "installing contract -> ERC-20"
log "... network = $net"
log "... node = $node"
log "... node address = "$node_address
log "contract constructor args:"
log "... token name = "$token_name
log "... token symbol = "$token_symbol
log "... token supply = "$token_supply
log "contract installation details:"
log "... path = "$contract_path
log "... deploy hash = "$deploy_hash
