#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Initialises scenario binaries.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _set_assets()
{
    local PATH_TO_STAGING=${1}
    local PATH_TO_STAGING_VERSION
    local SCENARIO_COMMIT_HASH
    local SCENARIO_VERSION
    local STAGING_TARGET
    local UPGRADE_STAGING_TARGET
    local IFS

    log "... setting assets"

    pushd "$PATH_TO_STAGING/src" || exit

    for UPGRADE_STAGING_TARGET in "${NCTL_STAGING_TARGETS[@]}"
    do        
        # Destructure settings.
        IFS=':'
        read -ra STAGING_TARGET <<< "$UPGRADE_STAGING_TARGET"
        SCENARIO_COMMIT_HASH="${STAGING_TARGET[1]}"
        SCENARIO_VERSION="${STAGING_TARGET[0]}"
        PATH_TO_STAGING_VERSION="$PATH_TO_STAGING/$SCENARIO_VERSION"
        log "... ... $SCENARIO_VERSION"

        # Set commit.
        git checkout "$SCENARIO_COMMIT_HASH" > /dev/null 2>&1

        # Compile binaries.
        # if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        #     cargo build --package casper-node
        #     cargo build --package casper-client
        # else
        #     cargo build --release --package casper-node
        #     cargo build --release --package casper-client
        # fi
        # make build-contract-rs/add-bid
        # make build-contract-rs/delegate
        # make build-contract-rs/transfer-to-account-u512
        # make build-contract-rs/undelegate
        # make build-contract-rs/withdraw-bid
        # make build-contract-rs/activate-bid

        # Copy binaries -> staging.
        if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
            cp "./target/debug/casper-node" "$PATH_TO_STAGING_VERSION/bin"
            cp "./target/debug/casper-client" "$PATH_TO_STAGING_VERSION/bin-client"
        else
            cp "./target/release/casper-node" "$PATH_TO_STAGING_VERSION/bin"
            cp "./target/release/casper-client" "$PATH_TO_STAGING_VERSION/bin-client"
        fi

        # Copy wasm -> staging.
        for CONTRACT in "${NCTL_CONTRACTS_CLIENT_AUCTION[@]}"
        do
            cp "./target/wasm32-unknown-unknown/release/$CONTRACT" \
               "$PATH_TO_STAGING_VERSION/bin-client/auction"
        done  
        for CONTRACT in "${NCTL_CONTRACTS_CLIENT_TRANSFERS[@]}"
        do
            cp "./target/wasm32-unknown-unknown/release/$CONTRACT" \
                "$PATH_TO_STAGING_VERSION/bin-client/transfers"
        done         

        # Copy chainspec -> staging.
        cp "./resources/local/chainspec.toml.in" "$PATH_TO_STAGING_VERSION/chainspec/chainspec.toml"

        # Copy node config -> staging.
        cp "./resources/local/config.toml" "$PATH_TO_STAGING_VERSION/config"
    done

    popd || exit
}

#######################################
# Initialises file system with scenario directories.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _set_fs()
{
    local PATH_TO_STAGING=${1}
    local PATH_TO_STAGING_VERSION
    local SCENARIO_VERSION
    local UPGRADE_STAGING_TARGET
    local IFS

    log "... setting directories"

    # Set source code directory.
    if [ ! -d "$PATH_TO_STAGING/src" ]; then
        mkdir "$PATH_TO_STAGING/src"
        git clone "https://github.com/CasperLabs/casper-node.git" "$PATH_TO_STAGING/src" > /dev/null 2>&1
    fi

    # Set protocol version staging directories.
    for UPGRADE_STAGING_TARGET in "${NCTL_STAGING_TARGETS[@]}"
    do
        # Destructure settings.
        IFS=':'
        read -ra STAGING_TARGET <<< "$UPGRADE_STAGING_TARGET"
        SCENARIO_VERSION="${STAGING_TARGET[0]}"
        log "... ... $SCENARIO_VERSION"

        # Set directories.
        PATH_TO_STAGING_VERSION="$PATH_TO_STAGING/$SCENARIO_VERSION"
        if [ ! -d "$PATH_TO_STAGING_VERSION" ]; then
            mkdir "$PATH_TO_STAGING_VERSION"
            mkdir "$PATH_TO_STAGING_VERSION/bin"
            mkdir "$PATH_TO_STAGING_VERSION/bin-client"
            mkdir "$PATH_TO_STAGING_VERSION/bin-client/auction"
            mkdir "$PATH_TO_STAGING_VERSION/bin-client/eco"
            mkdir "$PATH_TO_STAGING_VERSION/bin-client/transfers"
            mkdir "$PATH_TO_STAGING_VERSION/chainspec"
            mkdir "$PATH_TO_STAGING_VERSION/config"
        fi
    done
}

#######################################
# Prepares assets for upgrade scenarios.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _main()
{
    log "staging: STARTS"

    local PATH_TO_STAGING="$NCTL/staging"

    # Bring settings into memory.
    source "$PATH_TO_STAGING/settings.sh"

    # Prepare file system.
    _set_fs "$PATH_TO_STAGING"

    # Set staged assets.
    _set_assets "$PATH_TO_STAGING"    

    log "staging: COMPLETE"
}

_main
