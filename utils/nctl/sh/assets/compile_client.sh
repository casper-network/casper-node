#!/usr/bin/env bash
#
#######################################
# Compiles client software.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_HOME - path to casper node repo.
#   NCTL_CASPER_CLIENT_HOME - path to casper client repo.
#   NCTL_COMPILE_TARGET - flag indicating whether software compilation target is release | debug.
########################################

# Import utils.
source "$NCTL"/sh/utils/main.sh

# Build client side contracts.
pushd "$NCTL_CASPER_HOME" || exit

# Make sure toolchains up to date
make setup-rs

make build-contract-rs/activate-bid
make build-contract-rs/add-bid
make build-contract-rs/delegate
make build-contract-rs/named-purse-payment
make build-contract-rs/transfer-to-account-u512
make build-contract-rs/undelegate
make build-contract-rs/withdraw-bid
make build-contract-rs/nctl-dictionary

popd || exit

unset OPTIND #clean OPTIND envvar, otherwise getopts can break.
COMPILE_MODE="release" #default compile mode to release.

# Build client utility.
pushd "$NCTL_CASPER_CLIENT_HOME" || \
    { echo "Could not find the casper-client-rs repo - have you cloned it into your working directory?"; exit; }

while getopts 'd' opt; do 
    case $opt in
        d ) 
            echo "-d client"
            COMPILE_MODE="debug" ;;
        * ) ;; #ignore other cl flags

    esac
done 

if [ "$NCTL_COMPILE_TARGET" = "debug" ] || [ "$COMPILE_MODE" = "debug" ]; then
    cargo build
else
    cargo build --release
fi

unset OPTIND
unset COMPILE_MODE #clean all envvar garbage we may have produced. 

popd || exit

# Build client utility.
pushd "$NCTL_CASPER_CLIENT_HOME" || exit

if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build
else
    cargo build --release
fi

popd || exit
