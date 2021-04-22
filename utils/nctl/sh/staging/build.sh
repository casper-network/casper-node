#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset STAGE_SOURCE
unset STAGE_ID
unset STAGE_PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        source) STAGE_SOURCE=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        version) STAGE_PROTOCOL_VERSION=${VALUE} ;;
        *)
    esac
done

STAGE_ID="${STAGE_ID:-1}"
STAGE_PROTOCOL_VERSION="${STAGE_PROTOCOL_VERSION:-"all"}"
STAGE_SOURCE="${STAGE_SOURCE:-"local"}"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Builds binaries.
# Arguments:
#   Stage protocol hash.
#######################################
function _set_binaries()
{
    local STAGE_SOURCE=${1}
    
    # Set commit.
    if [ "$STAGE_SOURCE" != "local" ]; then
        git checkout "$STAGE_SOURCE" > /dev/null 2>&1
    fi

    # Set node binary.
    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cargo build --package casper-node
    else
        cargo build --release --package casper-node
    fi

    # Set client binary.
    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cargo build --package casper-client
    else
        cargo build --release --package casper-client
    fi

    # Set client-side wasm.
    make build-contract-rs/add-bid
    make build-contract-rs/delegate
    make build-contract-rs/transfer-to-account-u512
    make build-contract-rs/undelegate
    make build-contract-rs/withdraw-bid
    make build-contract-rs/activate-bid
}

#######################################
# Stages assets.
# Arguments:
#   Path to stage folder.
#######################################
function _set_fileset()
{
    local PATH_TO_STAGE=${1}

    # Stage binaries.
    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cp "./target/debug/casper-client" \
           "$PATH_TO_STAGE/bin"
        cp "./target/debug/casper-node" \
           "$PATH_TO_STAGE/bin"
    else
        cp "./target/release/casper-client" \
           "$PATH_TO_STAGE/bin"
        cp "./target/release/casper-node" \
           "$PATH_TO_STAGE/bin"
    fi

    # Stage wasm.
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_AUCTION[@]}"
    do
        cp "./target/wasm32-unknown-unknown/release/$CONTRACT" \
           "$PATH_TO_STAGE/bin/wasm"
    done  
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_TRANSFERS[@]}"
    do
        cp "./target/wasm32-unknown-unknown/release/$CONTRACT" \
           "$PATH_TO_STAGE/bin/wasm"
    done  

    # Stage chainspec.
    cp "./resources/local/chainspec.toml.in" \
       "$PATH_TO_STAGE/resources/chainspec.toml"

    # Stage node config.
    cp "./resources/local/config.toml" \
       "$PATH_TO_STAGE/resources" 
}

#######################################
# Prepares & states assets.
# Arguments:
#   Path to stage folder.
#######################################
function _set_assets()
{
    log "... setting assets"

    local STAGE_PROTOCOL_VERSION=${1}
    local STAGE_SOURCE=${2}
    local PATH_TO_STAGE=${3}

    local PATH_TO_SOURCE
    local PROTOCOL_HASH
    local PROTOCOL_VERSION
    local STAGE_TARGET
    local IFS=':'
    
    # Set source code folder.
    if [ "$STAGE_SOURCE" == "local" ]; then
        PATH_TO_SOURCE="$NCTL_CASPER_HOME"
    else
        PATH_TO_SOURCE="$(get_path_to_temp_node)"
    fi
    pushd "$PATH_TO_SOURCE" || exit

    # Set specific commit by hash.
    if [ "$STAGE_SOURCE" != "local" ]; then
        git checkout "$STAGE_SOURCE" > /dev/null 2>&1
    fi

    # Build binaries.
    _set_binaries

    # Stage assets.
    _set_fileset "$PATH_TO_STAGE/$STAGE_PROTOCOL_VERSION"

    popd || exit
}

#######################################
# Ensures casper-node source code is ready to build.
#######################################
function _set_prerequisites_1()
{
    local STAGE_SOURCE=${1}

    if [ "$STAGE_SOURCE" != "local" ]; then
        if [ ! -d "$(get_path_to_temp_node)" ]; then
            mkdir "$(get_path_to_temp_node)"
            git clone "https://github.com/CasperLabs/casper-node.git" "$(get_path_to_temp_node)" > /dev/null 2>&1
        else
            pushd "$(get_path_to_temp_node)" || exit
            git fetch --all > /dev/null 2>&1
            git pull > /dev/null 2>&1
            popd || exit
        fi
    fi
}

#######################################
# Initialises file system with stage directories.
# Arguments:
#   Path to stage folder.
#######################################
function _set_prerequisites_2()
{
    local PROTOCOL_VERSION=${1}
    local PATH_TO_STAGE=${2}

    if [ ! -d "$PATH_TO_STAGE/$PROTOCOL_VERSION" ]; then
        mkdir "$PATH_TO_STAGE/$PROTOCOL_VERSION"
        mkdir "$PATH_TO_STAGE/$PROTOCOL_VERSION/bin"
        mkdir "$PATH_TO_STAGE/$PROTOCOL_VERSION/bin/wasm"
        mkdir "$PATH_TO_STAGE/$PROTOCOL_VERSION/resources"
    fi            
}

#######################################
# Builds assets for staging.
# Arguments:
#   Scenario ordinal identifier.
#   Scenario protocol version.
#   Scenario node code source.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local STAGE_PROTOCOL_VERSION=${2}
    local STAGE_SOURCE=${3}

    log "staging -> STARTS"
    log "... id=$STAGE_ID :: version=$STAGE_PROTOCOL_VERSION :: source=$STAGE_SOURCE"

    local PATH_TO_STAGE=$(get_path_to_stage "$STAGE_ID")

    # Set prerequisites.
    log "... setting pre-requisites"
    _set_prerequisites_1 "$STAGE_SOURCE"
    _set_prerequisites_2 "$STAGE_PROTOCOL_VERSION" "$PATH_TO_STAGE" 

    # Set stage.
    _set_assets "$STAGE_PROTOCOL_VERSION" "$STAGE_SOURCE" "$PATH_TO_STAGE"

    log "staging -> COMPLETE"
}

_main "$STAGE_ID" "$STAGE_PROTOCOL_VERSION" "$STAGE_SOURCE"
