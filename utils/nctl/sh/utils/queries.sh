#######################################
# Returns a state root hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifer.
#   Node ordinal identifer.
#   Block identifer.
#######################################
function get_main_purse_uref() {
echo $(
        source $NCTL/sh/views/view_chain_account.sh net=$1 root-hash=$2 account-key=$3 \
            | jq '.stored_value.Account.main_purse' \
            | sed -e 's/^"//' -e 's/"$//'
    )
}

#######################################
# Returns a state root hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifer.
#   Node ordinal identifer.
#   Block identifer.
#######################################
function get_state_root_hash() {
    node_address=$(get_node_address_json $1 $2)
    if [ "$3" ]; then
        $NCTL/assets/net-$net/bin/casper-client get-state-root-hash \
            --node-address $node_address \
            --block-identifier $3 \
            | jq '.result.state_root_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    else
        $NCTL/assets/net-$net/bin/casper-client get-state-root-hash \
            --node-address $node_address \
            | jq '.result.state_root_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    fi
}

