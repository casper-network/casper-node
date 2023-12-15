#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/assets/setup_shared.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Sets network directories.
# Arguments:
#   Count of nodes to setup (default=5).
#   Version of protocol to which system is being upgraded.
#######################################
function _set_directories()
{
    log "... setting directories"

    local NODE_ID=${1}
    local PROTOCOL_VERSION=${2}

    mkdir -p "$(get_path_to_node_bin $NODE_ID)/$PROTOCOL_VERSION"
    mkdir -p "$(get_path_to_node_config $NODE_ID)/$PROTOCOL_VERSION"
}

#######################################
# Sets network binaries.
# Arguments:
#   Version of protocol being run.
#   Count of nodes to setup (default=5).
#   Path to casper-client binary.
#   Path to casper-node binary.
#   Path to casper-node-launcher binary.
#   Path to folder containing wasm binaries.
#######################################
function _setup_asset_binaries()
{
    log "... setting binaries"

    local PROTOCOL_VERSION=${1}
    local NODE_ID=${2}
    local PATH_TO_CLIENT=${3}
    local PATH_TO_NODE=${4}
    local PATH_TO_NODE_LAUNCHER=${5}
    local PATH_TO_SIDECAR=${6}
    local PATH_TO_WASM=${7}

    local PATH_TO_BIN
    local CONTRACT

    # Set node binaries.
    PATH_TO_BIN="$(get_path_to_node_bin "$NODE_ID")"
    if [ ! -f "$PATH_TO_BIN/casper-node-launcher" ]; then
        cp "$PATH_TO_NODE_LAUNCHER" "$PATH_TO_BIN"
    fi
    cp "$PATH_TO_NODE" "$PATH_TO_BIN/$PROTOCOL_VERSION"

    if [ -f "$PATH_TO_SIDECAR" ]; then
        cp "$PATH_TO_SIDECAR" "$PATH_TO_BIN/$PROTOCOL_VERSION"
    fi

    # Set client-side binary.
    PATH_TO_BIN="$(get_path_to_net)/bin"
    cp "$PATH_TO_CLIENT" "$PATH_TO_BIN"

    # Set client-side auction contracts;
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_AUCTION[@]}"
    do
        if [ -f "$PATH_TO_WASM/$CONTRACT" ]; then
            cp "$PATH_TO_WASM/$CONTRACT" \
               "$PATH_TO_BIN/auction"
        fi
    done

    # Set client-side shared contracts;
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_SHARED[@]}"
    do
        if [ -f "$PATH_TO_WASM/$CONTRACT" ]; then
            cp "$PATH_TO_WASM/$CONTRACT" \
            "$PATH_TO_BIN/shared"
        fi
    done

    # Set client-side transfer contracts;
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_TRANSFERS[@]}"
    do
        if [ -f "$PATH_TO_WASM/$CONTRACT" ]; then
            cp "$PATH_TO_WASM/$CONTRACT" \
            "$PATH_TO_BIN/transfers"
        fi
    done
}

#######################################
# Sets network chainspec.
# Arguments:
#   Count of nodes to setup (default=5).
#   Point (timestamp | era-id) when chainspec is considered live.
#   Delay in seconds to apply to genesis timestamp.
#   Path to chainspec template file.
#   Flag indicating whether chainspec pertains to genesis.
#######################################
function _setup_asset_chainspec()
{
    log "... setting chainspec.toml"

    local PROTOCOL_VERSION=${1}
    local ACTIVATION_POINT=${2}
    local PATH_TO_CHAINSPEC_TEMPLATE=${3}
    local IS_GENESIS=${4}
    local PATH_TO_CHAINSPEC
    local SCRIPT
    local COUNT_NODES

    # Use # of nodes in assets dir
    COUNT_NODES="$(get_count_of_nodes)"

    # Set file.
    PATH_TO_CHAINSPEC="$(get_path_to_net)/chainspec/chainspec.toml"
    cp "$PATH_TO_CHAINSPEC_TEMPLATE" "$PATH_TO_CHAINSPEC"

    # Using sed because toml.dump was adding quotes around the true
    # which caused issues.
    sed -i 's/hard_reset = false/hard_reset = true/g' "$PATH_TO_CHAINSPEC"

    # Set contents.
    if [ "$IS_GENESIS" == true ]; then
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CHAINSPEC');"
            "cfg['protocol']['activation_point']='$ACTIVATION_POINT';"
            "cfg['protocol']['version']='$PROTOCOL_VERSION';"
            "cfg['network']['name']='$(get_chain_name)';"
            "cfg['core']['validator_slots']=$COUNT_NODES;"
            "toml.dump(cfg, open('$PATH_TO_CHAINSPEC', 'w'));"
        )
    else
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CHAINSPEC');"
            "cfg['protocol']['activation_point']=$ACTIVATION_POINT;"
            "cfg['protocol']['version']='$PROTOCOL_VERSION';"
            "cfg['network']['name']='$(get_chain_name)';"
            "cfg['core']['validator_slots']=$COUNT_NODES;"
            "toml.dump(cfg, open('$PATH_TO_CHAINSPEC', 'w'));"
        )
    fi

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Sets node configuration files.
# Arguments:
#   Count of nodes to setup (default=5).
#   Version of protocol being ran.
#   Path to node configuration template file.
#   Flag indicating whether chainspec pertains to genesis.
#######################################
function _setup_asset_node_configs()
{
    log "... setting node configs"

    local NODE_ID=${1}
    local PROTOCOL_VERSION=${2}
    local PATH_TO_TEMPLATE=${3}
    local PATH_TO_SIDECAR_TEMPLATE=${4}
    local IS_GENESIS=${5}

    local PATH_TO_NET
    local PATH_TO_CONFIG
    local PATH_TO_CONFIG_FILE
    local PATH_TO_SIDECAR_CONFIG_FILE
    local SPECULATIVE_EXEC_ADDR
    local SCRIPT

    PATH_TO_NET="$(get_path_to_net)"

    # Set paths to node's config.
    PATH_TO_CONFIG="$(get_path_to_node "$NODE_ID")/config/$PROTOCOL_VERSION"
    PATH_TO_CONFIG_FILE="$PATH_TO_CONFIG/config.toml"
    PATH_TO_SIDECAR_CONFIG_FILE="$PATH_TO_CONFIG/sidecar.toml"

    # Set node configuration.
    if [ "$IS_GENESIS" == true ]; then
        cp "$PATH_TO_NET/chainspec/accounts.toml" "$PATH_TO_CONFIG"
    fi
    cp "$PATH_TO_NET/chainspec/chainspec.toml" "$PATH_TO_CONFIG"
    cp "$PATH_TO_TEMPLATE" "$PATH_TO_CONFIG_FILE"

    SPECULATIVE_EXEC_ADDR=$(grep 'speculative_exec_server' $PATH_TO_CONFIG_FILE || true)

    # Set node configuration settings.
    SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CONFIG_FILE');"
        "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
        "cfg['logging']['format']='$NCTL_NODE_LOG_FORMAT';"
        "cfg['network']['bind_address']='$(get_network_bind_address "$NODE_ID")';"
        "cfg['network']['known_addresses']=[$(get_network_known_addresses "$NODE_ID")];"
        "cfg['storage']['path']='../../storage';"
        "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$NODE_ID")';"
        "cfg['binary_port_server']['address']='0.0.0.0:$(get_node_port_binary "$NODE_ID")';"
        "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$NODE_ID")';"
    )

    SCRIPT+=(
        "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
    )

    if [ -f "$PATH_TO_SIDECAR_TEMPLATE" ]; then
        # Prepare the sidecar config file.
        cp "$PATH_TO_SIDECAR_TEMPLATE" "$PATH_TO_SIDECAR_CONFIG_FILE"

            SCRIPT+=(
            "cfg=toml.load('$PATH_TO_SIDECAR_CONFIG_FILE');"
            "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$NODE_ID")';"
            "cfg['speculative_exec_server']['address']='0.0.0.0:$(get_node_port_speculative_exec "$NODE_ID")';"
            "cfg['node_client']['address']='0.0.0.0:$(get_node_port_binary "$NODE_ID")';"
            "toml.dump(cfg, open('$PATH_TO_SIDECAR_CONFIG_FILE', 'w'));"
        )
    fi

    python3 -c "${SCRIPT[*]}"

    # Do workarounds.
    # N.B. - these are temporary & come into scope when testing against protocol versions
    #        that have conflicting node configuration schemas.
    _setup_asset_node_config_workaround_1 "$NODE_ID" "$PATH_TO_CONFIG_FILE"

    # unit_hashes_folder was removed in version 1.4.2
    if [ "$(echo $PROTOCOL_VERSION | tr -d '_')" -ge "142" ]; then
        sed -i '/unit_hashes_folder/d' "$PATH_TO_CONFIG_FILE"
    fi
}

#######################################
# Sets node configuration file workaround related to 'unit_hashes_folder' setting change.
# Arguments:
#   Node ordinal identifier.
#   Path to folder containing staged config files.
#######################################
function _setup_asset_node_config_workaround_1()
{
    local NODE_ID=${1}
    local PATH_TO_CONFIG_FILE=${2}
    local HAS_HIGHWAY
    local SCRIPT

    HAS_HIGHWAY=$(grep -R "consensus.highway" "$PATH_TO_CONFIG_FILE" || true)
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

function _setup_asset_global_state_toml() {
    log "... setting node global_state.toml"

    local IDX=${1}
    local TARGET_PROTOCOL_VERSION
    local VERSION_14_BOUNDARY

    local SEMVER_GLOBAL_STATE_UPDATE_TOOL
    local SEMVER_STAGE_1_PRE_VERSION

    read -ra SEMVER_GLOBAL_STATE_UPDATE_TOOL <<< "$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen --version | grep -oE '[^ ]+$' | tr '.' ' ')"

    # Check version of the global state update tool. 0.3 marks the transition to 1.5 fast sync node and supports the "validators" command.
    if [ "${SEMVER_GLOBAL_STATE_UPDATE_TOOL[1]}" -lt "3" ]; then
        log "ERROR :: Global State Update Generator must be version 0.3 or greater (found version ${SEMVER_GLOBAL_STATE_UPDATE_TOOL[0]}.${SEMVER_GLOBAL_STATE_UPDATE_TOOL[1]}.${SEMVER_GLOBAL_STATE_UPDATE_TOOL[2]})"
        exit 1
    fi

    pushd "$(get_path_to_stages)/stage-1/"
    read -ra SEMVER_STAGE_1_PRE_VERSION <<< $(find ./* -maxdepth 0 -type d | awk -F'/' '{ print $2 }' |  sort | head -n 1 | tr '_' ' ')
    popd
    pushd "$(get_path_to_stages)/stage-1/"
    read -ra SEMVER_STAGE_1_POST_VERSION <<< $(find ./* -maxdepth 0 -type d | awk -F'/' '{ print $2 }' |  sort | tail -n 1 | tr '_' ' ')
    local TARGET_PROTOCOL_VERSION=$(find ./* -maxdepth 0 -type d | awk -F'/' '{ print $2 }' | sort | tail -n 1)
    popd

    log "... processing upgrade from ${SEMVER_STAGE_1_PRE_VERSION[0]}.${SEMVER_STAGE_1_PRE_VERSION[1]}.${SEMVER_STAGE_1_PRE_VERSION[2]} to ${SEMVER_STAGE_1_POST_VERSION[0]}.${SEMVER_STAGE_1_POST_VERSION[1]}.${SEMVER_STAGE_1_POST_VERSION[2]}"

    if [ "${SEMVER_STAGE_1_PRE_VERSION[0]}" -le "1" ] && \
       [ "${SEMVER_STAGE_1_PRE_VERSION[1]}" -lt "4" ] && \
       [ "${SEMVER_STAGE_1_POST_VERSION[0]}" -ge "1" ] && \
       [ "${SEMVER_STAGE_1_POST_VERSION[1]}" -ge "4" ]; then
        log "... upgrading across the 1.4.0 boundary, generating 'global_state.toml' file"
        VERSION_14_BOUNDARY=1
    else
        log "... not upgrading across the 1.4.0 boundary, no 'global_state.toml' file needed"
        VERSION_14_BOUNDARY=0
    fi

    if [ "$VERSION_14_BOUNDARY" -eq "1" ]; then
        setup_asset_global_state_toml_for_node $IDX $TARGET_PROTOCOL_VERSION
    fi
}

#######################################
# Returns next version to which protocol will be upgraded.
# Arguments:
#   Path to folder containing staged files.
#######################################
function _get_protocol_version_of_next_upgrade()
{
    local PATH_TO_STAGE=${1}
    local NODE_ID=${2}
    local IFS='_'
    local PROTOCOL_VERSION
    local PATH_TO_NX_BIN
    local SEMVAR_CURRENT
    local SEMVAR_NEXT

    PATH_TO_NX_BIN="$(get_path_to_net)/nodes/node-$NODE_ID/bin"

    # Set semvar of current version.
    pushd "$PATH_TO_NX_BIN" || exit
    read -ra SEMVAR_CURRENT <<< "$(ls -td -- * | head -n 1)"
    popd || exit

    # Iterate staged bin directories and return first whose semvar > current.
    for FHANDLE in "$PATH_TO_STAGE/"*; do
        if [ -d "$FHANDLE" ]; then
            PROTOCOL_VERSION=$(basename "$FHANDLE")
            if [ ! -d "$PATH_TO_NX_BIN/$PROTOCOL_VERSION" ]; then
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
# Moves upgrade assets into location.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}
    local VERBOSE=${3}
    local NODE_ID=${4}
    local CHAINSPEC_PATH=${5}
    local CONFIG_PATH=${6}
    local SIDECAR_CONFIG_PATH=${7}
    local PATH_TO_STAGE
    local PROTOCOL_VERSION

    PATH_TO_STAGE="$NCTL/stages/stage-$STAGE_ID"
    PROTOCOL_VERSION=$(_get_protocol_version_of_next_upgrade "$PATH_TO_STAGE" "$NODE_ID")

    if [ -z "$CHAINSPEC_PATH" ]; then
        CHAINSPEC_PATH="$PATH_TO_STAGE/$PROTOCOL_VERSION/chainspec.toml"
    fi

    if [ -z "$CONFIG_PATH" ]; then
        CONFIG_PATH="$PATH_TO_STAGE/$PROTOCOL_VERSION/config.toml"
    fi

    if [ -z "$SIDECAR_CONFIG_PATH" ]; then
        SIDECAR_CONFIG_PATH="$PATH_TO_STAGE/$PROTOCOL_VERSION/sidecar.toml"
    fi

    if [ "$PROTOCOL_VERSION" != "" ]; then
        if [ "$VERBOSE" == true ]; then
            log "stage $STAGE_ID :: upgrade assets -> $PROTOCOL_VERSION @ era $ACTIVATION_POINT"
        fi
        _set_directories "$NODE_ID" \
                         "$PROTOCOL_VERSION"
        _setup_asset_binaries "$PROTOCOL_VERSION" \
                             "$NODE_ID" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION/casper-client" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION/casper-node" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION/casper-node-launcher" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION/casper-rpc-sidecar" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION"
        _setup_asset_chainspec "$(get_protocol_version_for_chainspec "$PROTOCOL_VERSION")" \
                              "$ACTIVATION_POINT" \
                              "$CHAINSPEC_PATH" \
                              false
        _setup_asset_node_configs "$NODE_ID" \
                                 "$PROTOCOL_VERSION" \
                                 "$CONFIG_PATH" \
                                 "$SIDECAR_CONFIG_PATH" \
                                 false
        if [ "$(echo $PROTOCOL_VERSION | tr -d '_')" -ge "140" ]; then
            _setup_asset_global_state_toml "$NODE_ID" \
                                          "$PROTOCOL_VERSION"
        fi
        sleep 1.0
    else
        log "ATTENTION :: no more staged upgrades to rollout !!!"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset ACTIVATION_POINT
unset NET_ID
unset STAGE_ID
unset VERBOSE
unset NODE_ID
unset CHAINSPEC_PATH
unset CONFIG_PATH
unset SIDECAR_CONFIG_PATH

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        era) ACTIVATION_POINT=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        verbose) VERBOSE=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        chainspec_path) CHAINSPEC_PATH=${VALUE} ;;
        config_path) CONFIG_PATH=${VALUE} ;;
        sidecar_config_path) SIDECAR_CONFIG_PATH=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
CHAIN_ERA=$(get_chain_era)
DEFAULT_ACTIVATION_POINT=$((CHAIN_ERA + NCTL_DEFAULT_ERA_ACTIVATION_OFFSET))
ACTIVATION_POINT="${ACTIVATION_POINT:-$DEFAULT_ACTIVATION_POINT}"
if [ "$ACTIVATION_POINT" == "N/A" ]; then
    ACTIVATION_POINT=0
fi

_main "${STAGE_ID:-1}" \
      "${ACTIVATION_POINT}" \
      "${VERBOSE:-true}" \
      "${NODE_ID}" \
      "${CHAINSPEC_PATH}" \
      "${CONFIG_PATH}" \
      "${SIDECAR_CONFIG_PATH}"
