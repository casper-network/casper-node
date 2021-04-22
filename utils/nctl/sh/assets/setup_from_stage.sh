#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset PATH_TO_ACCOUNTS
unset GENESIS_DELAY_SECONDS
unset NET_ID
unset NODE_COUNT
unset STAGE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        accounts_path) PATH_TO_ACCOUNTS=${VALUE} ;;
        delay) GENESIS_DELAY_SECONDS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        nodes) NODE_COUNT=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
GENESIS_DELAY_SECONDS=${GENESIS_DELAY_SECONDS:-30}
NODE_COUNT=${NODE_COUNT:-5}
PATH_TO_ACCOUNTS=${PATH_TO_ACCOUNTS:-""}
STAGE_ID="${STAGE_ID:-1}"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Gets a node's default POS weight.
# Arguments:
#   Count of genesis nodes to setup (default=5).
#   Node ordinal identifier.
#######################################
function _get_node_pos_stake_weight()
{
    local COUNT_NODES_AT_GENESIS=${1}
    local NODE_ID=${2}
    local POS_WEIGHT

    if [ "$NODE_ID" -le "$COUNT_NODES_AT_GENESIS" ]; then
        POS_WEIGHT=$(get_node_staking_weight "$NODE_ID")
    else
        POS_WEIGHT="0"
    fi
    if [ "x$POS_WEIGHT" = 'x' ]; then
        POS_WEIGHT="0"
    fi

    echo $POS_WEIGHT
}

#######################################
# Sets network accounts.toml.
# Arguments:
#   Count of genesis nodes to setup (default=5).
#   Count of all nodes to setup (default=10).
#   Count of users to setup (default=5).
#######################################
function _set_accounts_1()
{
    log "... setting accounts.toml"

    local COUNT_NODES=${1}
    local COUNT_NODES_AT_GENESIS=${2}
    local COUNT_USERS=${3}

    local IDX
    local PATH_TO_NET=$(get_path_to_net)
    local PATH_TO_ACCOUNTS="$PATH_TO_NET"/chainspec/accounts.toml

    # Set accounts.toml.
    touch "$PATH_TO_ACCOUNTS"

    # Set faucet account entry.
    cat >> "$PATH_TO_ACCOUNTS" <<- EOM
# FAUCET.
[[accounts]]
public_key = "$(cat "$PATH_TO_NET/faucet/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_FAUCET"
EOM

    # Set validator account entries.
    for IDX in $(seq 1 "$COUNT_NODES")
    do
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# VALIDATOR $IDX.
[[accounts]]
public_key = "$(cat "$PATH_TO_NET/nodes/node-$IDX/keys/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_VALIDATOR"
EOM
        if [ "$IDX" -le "$COUNT_NODES_AT_GENESIS" ]; then
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

[accounts.validator]
bonded_amount = "$(_get_node_pos_stake_weight "$COUNT_NODES_AT_GENESIS" "$IDX")"
delegation_rate = $IDX
EOM
        fi
    done

    # Set user account entries.
    for IDX in $(seq 1 "$COUNT_USERS")
    do
        if [ "$IDX" -le "$COUNT_NODES_AT_GENESIS" ]; then
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# USER $IDX.
[[delegators]]
validator_public_key = "$(cat "$PATH_TO_NET/nodes/node-$IDX/keys/public_key_hex")"
delegator_public_key = "$(cat "$PATH_TO_NET/users/user-$IDX/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_USER"
delegated_amount = "$((NCTL_INITIAL_DELEGATION_AMOUNT + IDX))"
EOM
        else
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# USER $IDX.
[[accounts]]
public_key = "$(cat "$PATH_TO_NET/users/user-$IDX/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_USER"
EOM
        fi
    done
}

#######################################
# Sets network accounts.toml from an existing template.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#   Path to accounts.toml template file.
#######################################
function _set_accounts_2()
{
    log "... setting accounts.toml (from template)"    

    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local PATH_TO_TEMPLATE=${3}

    local ACCOUNT_KEY
    local PATH_TO_ACCOUNTS
    local PBK_KEY
    local IDX

    # Copy across template.    
    PATH_TO_ACCOUNTS="$(get_path_to_net)"/chainspec/accounts.toml
    cp "$PATH_TO_TEMPLATE" "$PATH_TO_ACCOUNTS"

    # Set faucet.
    PBK_KEY="PBK_FAUCET"
    ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")"    
    sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"

    # Set validators.
    for IDX in $(seq "$COUNT_NODES" -1 1 )
    do
        PBK_KEY=PBK_V"$IDX"
        ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$IDX")"
        sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"
    done
    
    # Set users.
    for IDX in $(seq "$COUNT_USERS" -1 1)
    do
        PBK_KEY=PBK_U"$IDX"
        ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$IDX")"
        sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"
    done
}

#######################################
# Sets network accounts.toml from an existing template.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#   Path to accounts.toml template file.
#######################################
function _set_accounts()
{
    local COUNT_NODES=${1}
    local COUNT_NODES_AT_GENESIS=${2}
    local COUNT_USERS=${3}
    local PATH_TO_TEMPLATE=${4}

    if [ "$PATH_TO_TEMPLATE" = "" ]; then
        _set_accounts_1 "$COUNT_NODES" "$COUNT_NODES_AT_GENESIS" "$COUNT_USERS"
    else
        _set_accounts_2 "$COUNT_NODES" "$COUNT_USERS" "$PATH_TO_TEMPLATE"
    fi
}

#######################################
# Sets network binaries.
# Arguments:
#   Count of nodes to setup (default=5).
#   Path to folder containing staged files.
#######################################
function _set_binaries()
{
    log "... setting binaries"

    local COUNT_NODES=${1}
    local PATH_TO_STAGE=${2}

    local PATH_TO_BIN_OF_NET="$(get_path_to_net)/bin"
    local PATH_TO_BIN_OF_NODE
    local CONTRACT
    local IDX

    # Set node binaries.
    for IDX in $(seq 1 $COUNT_NODES)
    do
        PATH_TO_BIN_OF_NODE="$(get_path_to_node_bin "$IDX")"
        cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/release/casper-node-launcher" \
           "$PATH_TO_BIN_OF_NODE"
        cp "$PATH_TO_STAGE/bin/casper-node" \
           "$PATH_TO_BIN_OF_NODE/1_0_0"
    done

    # Set client binary.
    cp "$PATH_TO_STAGE/bin/casper-client" "$PATH_TO_BIN_OF_NET"

    # Set client contracts.
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_AUCTION[@]}"
    do
        cp "$PATH_TO_STAGE/bin/wasm/$CONTRACT" "$PATH_TO_BIN_OF_NET/auction"
    done  
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_TRANSFERS[@]}"
    do
        cp "$PATH_TO_STAGE/bin/wasm/$CONTRACT" "$PATH_TO_BIN_OF_NET/transfers"
    done   
}

#######################################
# Sets network chainspec.
# Arguments:
#   Count of nodes to setup (default=5).
#   Delay in seconds to apply to genesis timestamp.
#   Path to chainspec template file.
#######################################
function _set_chainspec()
{
    log "... setting chainspec.toml"

    local COUNT_NODES=${1}
    local GENESIS_DELAY=${2}
    local PATH_TO_STAGE=${3}

    local PATH_TO_CHAINSPEC="$(get_path_to_net)/chainspec/chainspec.toml"
    local PATH_TO_STAGED_CHAINSPEC="$PATH_TO_STAGE/resources/chainspec.toml"
    local SCRIPT

    # Set file.
    cp "$PATH_TO_STAGED_CHAINSPEC" "$PATH_TO_CHAINSPEC"

    # Set contents.
    SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CHAINSPEC');"
        "cfg['protocol']['activation_point']='$(get_genesis_timestamp "$GENESIS_DELAY")';"
        "cfg['protocol']['version']='1.0.0';"
        "cfg['network']['name']='$(get_chain_name)';"
        "cfg['core']['validator_slots']=$COUNT_NODES;"
        "toml.dump(cfg, open('$PATH_TO_CHAINSPEC', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"
}

#######################################
# Sets network daemon configuration.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
#######################################
function _set_daemon()
{
    log "... setting daemon config"

    if [ "$NCTL_DAEMON_TYPE" = "supervisord" ]; then
        source "$NCTL/sh/assets/setup_supervisord.sh"
    fi
}

#######################################
# Sets network directories.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#######################################
function _set_directories()
{
    log "... setting directories"

    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local IDX
    local PATH_TO_NET="$(get_path_to_net)"
    local PATH_TO_NODE


    mkdir "$PATH_TO_NET/bin"
    mkdir "$PATH_TO_NET/bin/auction"
    mkdir "$PATH_TO_NET/bin/eco"
    mkdir "$PATH_TO_NET/bin/transfers"
    mkdir "$PATH_TO_NET/chainspec"
    mkdir "$PATH_TO_NET/daemon"
    mkdir "$PATH_TO_NET/daemon/config"
    mkdir "$PATH_TO_NET/daemon/logs"
    mkdir "$PATH_TO_NET/daemon/socket"
    mkdir "$PATH_TO_NET/faucet"
    mkdir "$PATH_TO_NET/nodes"
    mkdir "$PATH_TO_NET/users"

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        PATH_TO_NODE="$PATH_TO_NET/nodes/node-$IDX"
        mkdir "$PATH_TO_NODE"
        mkdir "$PATH_TO_NODE/bin"
        mkdir "$PATH_TO_NODE/bin/1_0_0"
        mkdir "$PATH_TO_NODE/config"
        mkdir "$PATH_TO_NODE/config/1_0_0"
        mkdir "$PATH_TO_NODE/keys"
        mkdir "$PATH_TO_NODE/logs"
        mkdir "$PATH_TO_NODE/storage"
        mkdir "$PATH_TO_NODE/storage-consensus"
    done
     
    for IDX in $(seq 1 "$COUNT_USERS")
    do
        mkdir "$PATH_TO_NET/users/user-$IDX"
    done
}

#######################################
# Sets network keys.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#######################################
function _set_keys()
{
    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local IDX
    local PATH_TO_CLIENT="$(get_path_to_client)"
    local PATH_TO_NET="$(get_path_to_net)"

    log "... setting cryptographic keys"

    "$PATH_TO_CLIENT" keygen -f "$PATH_TO_NET/faucet" > /dev/null 2>&1

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        "$PATH_TO_CLIENT" keygen -f "$PATH_TO_NET/nodes/node-$IDX/keys" > /dev/null 2>&1
    done
     
    for IDX in $(seq 1 "$COUNT_USERS")
    do
        "$PATH_TO_CLIENT" keygen -f "$PATH_TO_NET/users/user-$IDX" > /dev/null 2>&1
    done
}

#######################################
# Sets node confgiuration files.
# Arguments:
#   Count of nodes to setup (default=5).
#   Path to folder containing staged config files.
#######################################
function _set_node_configs()
{
    log "... setting node config"
    
    local COUNT_NODES=${1}
    local PATH_TO_STAGE=${2}

    local IDX
    local PATH_TO_NET="$(get_path_to_net)"
    local PATH_TO_CONFIG
    local PATH_TO_CONFIG_FILE

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        # Set path to node's config folder.
        PATH_TO_CONFIG="$(get_path_to_node "$IDX")/config/1_0_0"

        # Set node configuration assets.
        cp "$PATH_TO_NET/chainspec/accounts.toml" "$PATH_TO_CONFIG"
        cp "$PATH_TO_NET/chainspec/chainspec.toml" "$PATH_TO_CONFIG"
        cp "$PATH_TO_STAGE/resources/config.toml" "$PATH_TO_CONFIG"

        # Set node configuration settings.
        _set_node_config "$IDX" "$PATH_TO_CONFIG/config.toml"
    done
}

#######################################
# Sets node confgiuration files.
# Arguments:
#   Node ordinal identifier.
#   Path to folder containing staged config files.
#######################################
function _set_node_config()
{
    local NODE_ID=${1}
    local PATH_TO_CONFIG_FILE=${2}

    # Set node configuration file.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CONFIG_FILE');"
        "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
        "cfg['logging']['format']='$NCTL_NODE_LOG_FORMAT';"
        "cfg['network']['bind_address']='$(get_network_bind_address "$NODE_ID")';"
        "cfg['network']['known_addresses']=[$(get_network_known_addresses "$NODE_ID")];"
        "cfg['storage']['path']='../../storage';"
        "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$NODE_ID")';"
        "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$NODE_ID")';"
        "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$NODE_ID")';"
        "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    # Set node configuration file workarounds.
    _set_node_config_workaround_1 "$NODE_ID" "$PATH_TO_CONFIG_FILE"
}

#######################################
# Sets node configuration file workaround related to 'unit_hashes_folder' setting change.
# Arguments:
#   Node ordinal identifier.
#   Path to folder containing staged config files.
#######################################
function _set_node_config_workaround_1()
{
    local NODE_ID=${1}
    local PATH_TO_CONFIG_FILE=${2}

    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CONFIG_FILE');"
        "cfg['consensus']['highway']['unit_hashes_folder']='../../storage-consensus';"
        "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}" > /dev/null 2>&1

    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CONFIG_FILE');"
        "cfg['consensus']['unit_hashes_folder']='../../storage-consensus';"
        "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}" > /dev/null 2>&1
}

#######################################
# Tears down existing network nodes.
#######################################
function _unset_existing()
{
    local PATH_TO_NET=$(get_path_to_net)

    if [ -d "$PATH_TO_NET" ]; then
        source "$NCTL/sh/node/stop.sh"
        rm -rf "$PATH_TO_NET"
    fi
    mkdir -p "$PATH_TO_NET"
}

#######################################
# Prepares assets for upgrade scenario start.
# Arguments:
#   Network nodeset count.
#   Delay in seconds pripr to which genesis window will expire.
#   Path to template accounts.toml.
#   Scenario ordinal identifier.
#######################################
function _main()
{

    local STAGE_ID=${1}
    local COUNT_NODES_AT_GENESIS=${2}
    local COUNT_NODES=$(($COUNT_NODES_AT_GENESIS * 2))
    local GENESIS_DELAY=${3}
    local PATH_TO_ACCOUNTS=${4}

    local COUNT_USERS="$COUNT_NODES"
    local PATH_TO_STAGE="$NCTL/stages/stage-$STAGE_ID/1_0_0"
    local PATH_TO_STAGED_CHAINSPEC="$PATH_TO_STAGE/chainspec/chainspec.toml"

    log "setup of assets from stage ${1}: STARTS"

    _unset_existing
    _set_directories "$COUNT_NODES" "$COUNT_USERS"
    _set_binaries "$COUNT_NODES" "$PATH_TO_STAGE"
    _set_keys "$COUNT_NODES" "$COUNT_USERS"
    _set_daemon
    _set_chainspec "$COUNT_NODES" "$GENESIS_DELAY" "$PATH_TO_STAGE"
    _set_accounts "$COUNT_NODES" "$COUNT_NODES_AT_GENESIS" "$COUNT_USERS" "$PATH_TO_ACCOUNTS"
    _set_node_configs "$COUNT_NODES" "$PATH_TO_STAGE"

    log "setup of assets from stage ${1}: COMPLETE"
}

_main "$STAGE_ID" "$NODE_COUNT" "$GENESIS_DELAY_SECONDS" "$PATH_TO_ACCOUNTS"
