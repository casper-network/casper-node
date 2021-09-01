#!/usr/bin/env bash
#
#######################################
# Compiles client software.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_HOME - path to casper node repo.
#   NCTL_COMPILE_TARGET - flag indicating whether software compilation target is release | debug.
########################################

# Import utils.
source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_HOME" || exit

# Make sure toolchains up to date
make setup-rs

# Build client utility.
if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build --package casper-client --features casper-mainnet
else
    cargo build --release --package casper-client --features casper-mainnet
fi

# Build client side contracts.
make build-contract-rs/activate-bid
make build-contract-rs/add-bid
make build-contract-rs/delegate
make build-contract-rs/named-purse-payment
make build-contract-rs/transfer-to-account-u512
make build-contract-rs/undelegate
make build-contract-rs/withdraw-bid

popd || exit
