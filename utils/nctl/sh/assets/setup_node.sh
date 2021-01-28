#!/usr/bin/env bash

#######################################
# Sets assets pertaining to a single node.
# Globals:
#   NCTL_CASPER_HOME - path to node software github repo.
#   NCTL_VALIDATOR_BASE_WEIGHT - base weight applied to validator POS.
#   NCTL_INITIAL_BALANCE_VALIDATOR - initial balance of a lucky validator.
#   NCTL_ACCOUNT_TYPE_NODE - node account type enum.
# Arguments:
#   Node ordinal identifier.
#   Count of genesis nodes to initially setup.
#######################################
function setup_node()
{
    local NODE_ID=${1}
    local COUNT_GENESIS_NODES=${2}
    local PATH_TO_NET
    local PATH_TO_NODE

    PATH_TO_NET=$(get_path_to_net)
    PATH_TO_NODE=$(get_path_to_node "$NODE_ID")
    PATH_TO_NODE_CONFIG="$PATH_TO_NODE"/config/node-config.toml

    # Set directory.
    mkdir "$PATH_TO_NODE"
    mkdir "$PATH_TO_NODE"/config
    mkdir "$PATH_TO_NODE"/keys
    mkdir "$PATH_TO_NODE"/logs
    mkdir "$PATH_TO_NODE"/storage
    mkdir "$PATH_TO_NODE"/storage-consensus

    # Set config.
    cp "$NCTL_CASPER_HOME"/resources/local/config.toml "$PATH_TO_NODE_CONFIG"
    setup_node_config "$NODE_ID" "$PATH_TO_NODE_CONFIG"

    # Set keys.
    $(get_path_to_client) keygen -f "$PATH_TO_NODE"/keys > /dev/null 2>&1

    # Set staking weight.
    local POS_WEIGHT
    if [ "$NODE_ID" -le "$COUNT_GENESIS_NODES" ]; then
        POS_WEIGHT=$(get_node_staking_weight "$NODE_ID")
    else
        POS_WEIGHT=0
    fi

    # Set chainspec account.
	cat >> "$PATH_TO_NET"/chainspec/accounts.csv <<- EOM
	$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$NODE_ID"),$NCTL_INITIAL_BALANCE_VALIDATOR,$POS_WEIGHT
	EOM
}

#######################################
# Sets entry in node's config.toml.
# Arguments:
#   Node ordinal identifier.
#   Path node's config file.
#######################################
function setup_node_config()
{
    local NODE_ID=${1}
    local FILEPATH=${2}

    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$FILEPATH');"
        "cfg['consensus']['secret_key_path']='../keys/secret_key.pem';"
        "cfg['consensus']['unit_hashes_folder']='../storage-consensus';"
        "cfg['logging']['format']='json';"
        "cfg['network']['bind_address']='$(get_network_bind_address "$NODE_ID")';"
        "cfg['network']['known_addresses']=[$(get_network_known_addresses "$NODE_ID")];"
        "cfg['storage']['path']='../storage';"
        "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$NODE_ID")';"
        "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$NODE_ID")';"
        "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$NODE_ID")';"
        "toml.dump(cfg, open('$FILEPATH', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"
}

#######################################
# Sets chainspec assets pertaining to a single node.
# Globals:
#   NCTL_CASPER_HOME - path to node software github repo.
#   NCTL_VALIDATOR_BASE_WEIGHT - base weight applied to validator POS.
#   NCTL_INITIAL_BALANCE_VALIDATOR - initial balance of a lucky validator.
#   NCTL_ACCOUNT_TYPE_NODE - node account type enum.
# Arguments:
#   Node ordinal identifier.
#######################################
function setup_node_chainspec()
{
    local NODE_ID=${1}
    local PATH_TO_NET
    local PATH_TO_NODE

    PATH_TO_NET=$(get_path_to_net)
    PATH_TO_NODE=$(get_path_to_node "$NODE_ID")

    # Copy files.
    cp "$PATH_TO_NET"/chainspec/chainspec.toml "$PATH_TO_NODE"/config/chainspec.toml
    cp "$PATH_TO_NET"/chainspec/accounts.csv "$PATH_TO_NODE"/config/accounts.csv
}

