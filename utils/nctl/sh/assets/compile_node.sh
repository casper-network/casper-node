#!/usr/bin/env bash
#
#######################################
# Compiles node software.
# Globals:
#   NCTL_CASPER_HOME - path to casper node repo.
#   NCTL - path to nctl home directory.
########################################

# Import utils.
source $NCTL/sh/utils/main.sh

pushd $NCTL_CASPER_HOME
# make setup-rs
make build-system-contracts -j
cargo build --release --package casper-node
popd -1
