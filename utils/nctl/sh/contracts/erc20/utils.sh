# Import utils.
source $NCTL/sh/utils.sh

#######################################
# ERC-20: get on-chain contract hash.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Contract owner account key.
#######################################
function get_erc20_contract_hash () {
    node_address=$(get_node_address_rpc $1 $2)
    state_root_hash=$(get_state_root_hash $1 $2)
    path_client=$(get_path_to_client $1)
    echo $(
        $path_client query-state \
            --node-address $node_address \
            --state-root-hash $state_root_hash \
            --key $3 \
            | jq '.result.stored_value.Account.named_keys.ERC20.Hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
}

#######################################
# ERC-20: get on-chain contract key value.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Contract owner account key.
#   State query path.
#######################################
function get_erc20_contract_key_value () {
    node_address=$(get_node_address_rpc $1 $2)
    state_root_hash=$(get_state_root_hash $1 $2)
    path_client=$(get_path_to_client $1)
    echo $(
        $path_client query-state \
            --node-address $node_address \
            --state-root-hash $state_root_hash \
            --key $3 \
            --query-path $4 \
            | jq '.result.stored_value.CLValue.parsed_to_json' \
            | sed -e 's/^"//' -e 's/"$//'
        )
}
