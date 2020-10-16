#!/usr/bin/env bash
#
#######################################
# Compiles client software.
# Globals:
#   NCTL_CASPER_HOME - path to casper node repo.
#   NCTL - path to nctl home directory.
########################################

# Import utils.
source $NCTL/sh/utils/misc.sh

pushd $NCTL_CASPER_HOME

# Build client utility.
cargo build --release --package casper-client

# Build client side contracts.
make build-contract-rs/add-bid
make build-contract-rs/delegate
make build-contract-rs/transfer-to-account-u512
make build-contract-rs/transfer-to-account-u512-stored
make build-contract-rs/undelegate
make build-contract-rs/withdraw-bid

popd -1
