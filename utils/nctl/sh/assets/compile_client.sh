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

popd || exit

# Build client utility.
pushd "$NCTL_CASPER_CLIENT_HOME" || exit

if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build
else
    cargo build --release
fi

popd || exit
