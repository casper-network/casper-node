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

    # Set directory.
    mkdir "$PATH_TO_NODE"
    mkdir "$PATH_TO_NODE"/bin
    mkdir "$PATH_TO_NODE"/bin/1_0_0
    mkdir "$PATH_TO_NODE"/config
    mkdir "$PATH_TO_NODE"/config/1_0_0
    mkdir "$PATH_TO_NODE"/keys
    mkdir "$PATH_TO_NODE"/logs
    mkdir "$PATH_TO_NODE"/storage
    mkdir "$PATH_TO_NODE"/storage-consensus

    # Set bin.
    _setup_bin "$NODE_ID"

    # Set config.
    _setup_config "$NODE_ID"

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
	cat >> "$PATH_TO_NET"/chainspec/accounts.toml <<- EOM
[[accounts]]
public_key = "$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$NODE_ID")"
balance = "$NCTL_INITIAL_BALANCE_VALIDATOR"
staked_amount = "$POS_WEIGHT"
EOM
}

#######################################
# Sets binary assets pertaining to a single node.
# Globals:
#   NCTL_CASPER_HOME - path to node software github repo.
#   NCTL_CASPER_NODE_LAUNCHER_HOME - directory mapped to casper-node-launcher repo.
# Arguments:
#   Node ordinal identifier.
#######################################
function _setup_bin()
{
    local NODE_ID=${1}
    local PATH_TO_BIN
    local PATH_TO_BIN_SEMVAR

    PATH_TO_BIN=$(get_path_to_node_bin "$NODE_ID")
    PATH_TO_BIN_SEMVAR="$PATH_TO_BIN"/1_0_0

    cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/release/casper-node-launcher" "$PATH_TO_BIN"
    cp "$NCTL_CASPER_HOME"/target/release/casper-node "$PATH_TO_BIN_SEMVAR"
}

#######################################
# Sets entry in node's config.toml.
# Arguments:
#   Node ordinal identifier.
#   Path node's config file.
#######################################
function _setup_config()
{
    local NODE_ID=${1}
    local PATH_TO_FILE
    local PATH_TO_NODE

    PATH_TO_CFG=$(get_path_to_node "$NODE_ID")/config
    PATH_TO_CFG_SEMVAR="$PATH_TO_CFG"/1_0_0
    PATH_TO_FILE="$PATH_TO_CFG_SEMVAR"/config.toml

    cp "$NCTL_CASPER_HOME"/resources/local/config.toml "$PATH_TO_FILE"

    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_FILE');"
        "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
        "cfg['consensus']['unit_hashes_folder']='../../storage-consensus';"
        "cfg['logging']['format']='json';"
        "cfg['network']['bind_address']='$(get_network_bind_address "$NODE_ID")';"
        "cfg['network']['known_addresses']=[$(get_network_known_addresses "$NODE_ID")];"
        "cfg['storage']['path']='../../storage';"
        "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$NODE_ID")';"
        "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$NODE_ID")';"
        "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$NODE_ID")';"
        "toml.dump(cfg, open('$PATH_TO_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"
}
