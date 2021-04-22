#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset ACTIVATION_POINT
unset NET_ID
unset STAGE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        era) ACTIVATION_POINT=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
ACTIVATION_POINT="${ACTIVATION_POINT:-$(($(get_chain_era) + 1))}"
STAGE_ID="${STAGE_ID:-1}"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

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

    local CONTRACT
    local IDX
    local PATH_TO_BIN_OF_NET
    local PROTOCOL_VERSION

    PATH_TO_BIN_OF_NET="$(get_path_to_net)/bin"
    PROTOCOL_VERSION=$(basename "$PATH_TO_STAGE")

    # Set node binaries.
    for IDX in $(seq 1 "$COUNT_NODES")
    do
        cp "$PATH_TO_STAGE/bin/casper-node" \
           "$(get_path_to_net)/nodes/node-$IDX/bin/$PROTOCOL_VERSION"
    done

    # Set client binary.
    cp "$PATH_TO_STAGE/bin/casper-client" "$PATH_TO_BIN_OF_NET"

    # Set client side contracts.
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
    local PATH_TO_STAGE=${2}
    local ACTIVATION_POINT=${3}
    local PROTOCOL_VERSION
    local ACTIVATION_POINT
    local PATH_TO_CHAINSPEC
    local SCRIPT

    PROTOCOL_VERSION=$(basename "$PATH_TO_STAGE")
    PATH_TO_CHAINSPEC="$(get_path_to_net)/chainspec/chainspec.toml"

    # Set file.
    cp "$PATH_TO_STAGE/resources/chainspec.toml" "$PATH_TO_CHAINSPEC"

    # Set contents.
    PROTOCOL_VERSION=$(get_protocol_version_for_chainspec "$PROTOCOL_VERSION")
    SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CHAINSPEC');"
        "cfg['protocol']['activation_point']='$ACTIVATION_POINT';"
        "cfg['protocol']['version']='$PROTOCOL_VERSION';"
        "cfg['network']['name']='$(get_chain_name)';"
        "cfg['core']['validator_slots']=$COUNT_NODES;"
        "toml.dump(cfg, open('$PATH_TO_CHAINSPEC', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"
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
    local PROTOCOL_VERSION=${2}
    local IDX

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        mkdir -p "$(get_path_to_net)/nodes/node-$IDX/bin/$PROTOCOL_VERSION"
        mkdir -p "$(get_path_to_net)/nodes/node-$IDX/config/$PROTOCOL_VERSION"
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
    local PATH_TO_NET
    local PATH_TO_CONFIG
    local PROTOCOL_VERSION

    PATH_TO_NET="$(get_path_to_net)"
    PROTOCOL_VERSION=$(basename "$PATH_TO_STAGE")

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        PATH_TO_CONFIG="$(get_path_to_node "$IDX")/config/$PROTOCOL_VERSION"
        cp "$PATH_TO_NET/chainspec/chainspec.toml" "$PATH_TO_CONFIG"
        cp "$PATH_TO_STAGE/resources/config.toml" "$PATH_TO_CONFIG"
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
    local HAS_HIGHWAY
    local SCRIPT

    HAS_HIGHWAY=$(grep -R "consensus.highway" "$PATH_TO_CONFIG_FILE")
    if [ "$HAS_HIGHWAY" != "" ]; then
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CONFIG_FILE');"
            "cfg['consensus']['highway']['unit_hashes_folder']='../../storage-consensus';"
            "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
        )
    else
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CONFIG_FILE');"
            "cfg['consensus']['unit_hashes_folder']='../../storage-consensus';"
            "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
        )
    fi

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Returns next version to which protocol will be upgraded.
# Arguments:
#   Path to folder containing staged files.
#######################################
function _get_protocol_version_of_next_upgrade()
{
    local PATH_TO_STAGE=${1}
    local IFS='_'
    local PROTOCOL_VERSION
    local PATH_TO_N1_BIN
    local SEMVAR_CURRENT
    local SEMVAR_NEXT
    
    PATH_TO_N1_BIN="$(get_path_to_net)/nodes/node-1/bin"


    # Set semvar of current version.
    pushd "$PATH_TO_N1_BIN" || exit
    read -ra SEMVAR_CURRENT <<< "$(ls -td -- * | head -n 1)"
    popd || exit

    # Iterate staged bin directories and return first whose semvar > current.
    for FHANDLE in "$PATH_TO_STAGE/"*; do
        if [ -d "$FHANDLE" ]; then
            PROTOCOL_VERSION=$(basename "$FHANDLE")
            if [ ! -d "$PATH_TO_N1_BIN/$PROTOCOL_VERSION" ]; then
                read -ra SEMVAR_NEXT <<< "$PROTOCOL_VERSION"
                if [ "${SEMVAR_NEXT[0]}" -gt "${SEMVAR_CURRENT[0]}" ] || \
                   [ "${SEMVAR_NEXT[1]}" -gt "${SEMVAR_CURRENT[1]}" ] || \
                   [ "${SEMVAR_NEXT[2]}" -gt "${SEMVAR_CURRENT[2]}" ]; then
                    echo "$PROTOCOL_VERSION"
                    break
                fi
            fi
        fi
    done
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
    local ACTIVATION_POINT=${2}
    local PATH_TO_STAGE
    local PROTOCOL_VERSION
    local COUNT_NODES

    PATH_TO_STAGE="$NCTL/stages/stage-$STAGE_ID"
    COUNT_NODES=$(get_count_of_nodes)
    PROTOCOL_VERSION=$(_get_protocol_version_of_next_upgrade "$PATH_TO_STAGE")

    if [ "$PROTOCOL_VERSION" == "" ]; then
        log "ATTENTION :: no more staged upgrades to rollout !!!"
    else
        log "stage $STAGE_ID :: upgrade -> $PROTOCOL_VERSION @ era $ACTIVATION_POINT : STARTS"
        _set_directories "$COUNT_NODES" "$PROTOCOL_VERSION"
        _set_binaries "$COUNT_NODES" "$PATH_TO_STAGE/$PROTOCOL_VERSION"
        _set_chainspec "$COUNT_NODES" "$PATH_TO_STAGE/$PROTOCOL_VERSION" "$ACTIVATION_POINT"
        _set_node_configs "$COUNT_NODES" "$PATH_TO_STAGE/$PROTOCOL_VERSION"
        log "stage $STAGE_ID :: upgrade -> $PROTOCOL_VERSION @ era $ACTIVATION_POINT : COMPLETE"
    fi
}

_main "$STAGE_ID" "$ACTIVATION_POINT"
