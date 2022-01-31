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

    local COUNT_NODES=${1}
    local PROTOCOL_VERSION=${2}
    local IDX

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        mkdir -p "$(get_path_to_node_bin $IDX)/$PROTOCOL_VERSION"
        mkdir -p "$(get_path_to_node_config $IDX)/$PROTOCOL_VERSION"
    done
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
# Moves upgrade assets into location.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}
    local VERBOSE=${3}
    local CHUNKED_HASH_ACTIVATION
    local PATH_TO_STAGE
    local PROTOCOL_VERSION
    local COUNT_NODES

    #Set `verifiable_chunked_hash_activation` equal to upgrade activation point
    CHUNKED_HASH_ACTIVATION="$ACTIVATION_POINT"

    PATH_TO_STAGE="$NCTL/stages/stage-$STAGE_ID"
    COUNT_NODES=$(get_count_of_nodes)
    PROTOCOL_VERSION=$(_get_protocol_version_of_next_upgrade "$PATH_TO_STAGE")

    if [ "$PROTOCOL_VERSION" != "" ]; then
        if [ $VERBOSE == true ]; then
            log "stage $STAGE_ID :: upgrade assets -> $PROTOCOL_VERSION @ era $ACTIVATION_POINT"
        fi
        _set_directories "$COUNT_NODES" \
                         "$PROTOCOL_VERSION"
        setup_asset_binaries "$PROTOCOL_VERSION" \
                             "$COUNT_NODES" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION/casper-client" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION/casper-node" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION/casper-node-launcher" \
                             "$PATH_TO_STAGE/$PROTOCOL_VERSION"
        setup_asset_chainspec "$COUNT_NODES" \
                              "$(get_protocol_version_for_chainspec "$PROTOCOL_VERSION")" \
                              "$ACTIVATION_POINT" \
                              "$PATH_TO_STAGE/$PROTOCOL_VERSION/chainspec.toml" \
                              false \
                              "$CHUNKED_HASH_ACTIVATION"
        setup_asset_node_configs "$COUNT_NODES" \
                                 "$PROTOCOL_VERSION" \
                                 "$PATH_TO_STAGE/$PROTOCOL_VERSION/config.toml" \
                                 false
        setup_asset_global_state_toml "$COUNT_NODES" \
                                      "$PROTOCOL_VERSION"
        sleep 10.0
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

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        era) ACTIVATION_POINT=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        verbose) VERBOSE=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
ACTIVATION_POINT="${ACTIVATION_POINT:-$(get_chain_era)}"
if [ $ACTIVATION_POINT == "N/A" ]; then
    ACTIVATION_POINT=0
fi

_main "${STAGE_ID:-1}" \
      $((ACTIVATION_POINT + NCTL_DEFAULT_ERA_ACTIVATION_OFFSET)) \
      "${VERBOSE:-true}"
