#!/usr/bin/env bash

#######################################
# Prepares assets for network start.
# Arguments:
#   Network ordinal identifier.
#   Network nodeset count.
#   Delay in seconds pripr to which genesis window will expire.
#   Path to custom accounts.toml.
#   Path to custom chainspec.toml.
#######################################

#
# Sets assets required to run an N node network.
# Arguments:
#   Network ordinal identifier (default=1).
#   Count of nodes to setup (default=5).
#   Delay in seconds to apply to genesis timestamp (default=30).
#   Path to custom chain spec template file.

source "$NCTL"/sh/utils/main.sh
source "$NCTL/sh/assets/setup_shared.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset PATH_TO_ACCOUNTS
unset GENESIS_DELAY_SECONDS
unset NET_ID
unset NODE_COUNT
unset PATH_TO_CHAINSPEC

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        delay) GENESIS_DELAY_SECONDS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        nodes) NODE_COUNT=${VALUE} ;;
        chainspec_path) PATH_TO_CHAINSPEC=${VALUE} ;;
        accounts_path) PATH_TO_ACCOUNTS=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
GENESIS_DELAY_SECONDS=${GENESIS_DELAY_SECONDS:-30}
NODE_COUNT=${NODE_COUNT:-5}
PATH_TO_CHAINSPEC=${PATH_TO_CHAINSPEC:-"${NCTL_CASPER_HOME}/resources/local/chainspec.toml.in"}
PATH_TO_ACCOUNTS=${PATH_TO_ACCOUNTS:-""}


#######################################
# Sets network nodes.
#######################################
function _set_nodes()
{
    log "... setting node config"
    
    local IDX
    local PATH_TO_FILE
    local PATH_TO_NODE

    for IDX in $(seq 1 "$(get_count_of_nodes)")
    do
        PATH_TO_CFG=$(get_path_to_node "$IDX")/config/1_0_0
        PATH_TO_FILE="$PATH_TO_CFG"/config.toml

        cp "$NCTL_CASPER_HOME"/resources/local/config.toml "$PATH_TO_CFG"
        cp "$(get_path_to_net)"/chainspec/* "$PATH_TO_CFG"

        local SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_FILE');"
            "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
            "cfg['consensus']['highway']['unit_hashes_folder']='../../storage-consensus';"
            "cfg['logging']['format']='$NCTL_NODE_LOG_FORMAT';"
            "cfg['network']['bind_address']='$(get_network_bind_address "$IDX")';"
            "cfg['network']['known_addresses']=[$(get_network_known_addresses "$IDX")];"
            "cfg['storage']['path']='../../storage';"
            "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$IDX")';"
            "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$IDX")';"
            "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$IDX")';"
            "toml.dump(cfg, open('$PATH_TO_FILE', 'w'));"
        )
        python3 -c "${SCRIPT[*]}"
    done
}

#######################################
# Main
# Globals:
#   NET_ID - ordinal identifier of network being setup.
# Arguments:
#   Count of nodes to setup.
#   Delay in seconds to apply to genesis timestamp.
#   Path to template chainspec.
#   Path to template accounts.toml.
#######################################
function _main()
{
    local COUNT_NODES_AT_GENESIS=${1}
    local COUNT_NODES=$(($COUNT_NODES_AT_GENESIS * 2))
    local GENESIS_DELAY=${2}
    local PATH_TO_CHAINSPEC=${3}
    local PATH_TO_ACCOUNTS=${4}
    local COUNT_USERS="$COUNT_NODES"
    local PATH_TO_NET

    # Tear down previous.
    PATH_TO_NET=$(get_path_to_net)
    if [ -d "$PATH_TO_NET" ]; then
        source "$NCTL"/sh/assets/teardown.sh net="$NET_ID"
    fi
    mkdir -p "$PATH_TO_NET"

    log "asset setup begins ... please wait"

    # Setup new.
    # ... directories
    setup_asset_directories "$COUNT_NODES" "$COUNT_USERS"

    # ... binaries
    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        setup_asset_binaries "$(get_count_of_nodes)" \
                             "$NCTL_CASPER_HOME/target/debug/casper-client" \
                             "$NCTL_CASPER_HOME/target/debug/casper-node" \
                             "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/debug/casper-node-launcher" \
                             "$NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release"
    else
        setup_asset_binaries "$(get_count_of_nodes)" \
                             "$NCTL_CASPER_HOME/target/release/casper-client" \
                             "$NCTL_CASPER_HOME/target/release/casper-node" \
                             "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/release/casper-node-launcher" \
                             "$NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release"
    fi    

    # ... keys
    setup_asset_keys "$COUNT_NODES" "$COUNT_USERS"

    # ... daemon
    setup_asset_daemon
    
    # ... chainspec.toml
    setup_asset_chainspec "$COUNT_NODES" "$GENESIS_DELAY" "$PATH_TO_CHAINSPEC"

    # ... accounts.toml
    if [ "$PATH_TO_ACCOUNTS" = "" ]; then
        _set_accounts
    else
        _set_accounts_from_template "$PATH_TO_ACCOUNTS"
    fi

    if [ "$PATH_TO_ACCOUNTS" = "" ]; then
        setup_asset_accounts "$COUNT_NODES" "$COUNT_NODES_AT_GENESIS" "$COUNT_USERS"
    else
        setup_asset_accounts_from_template "$COUNT_NODES" "$COUNT_USERS" "$PATH_TO_ACCOUNTS"
    fi

    # ... nodes
    setup_asset_node_configs "$COUNT_NODES" "$NCTL_CASPER_HOME/resources/local/config.toml"

    log "asset setup complete"
}

_main "$NODE_COUNT" "$GENESIS_DELAY_SECONDS" "$PATH_TO_CHAINSPEC" "$PATH_TO_ACCOUNTS"
