#!/usr/bin/env bash
#
#######################################
# Compiles node launcher software.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_NODE_LAUNCHER_HOME - path to casper node launcher repo.
########################################

# Import utils.
source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_NODE_LAUNCHER_HOME" || exit

cargo build --release 

popd || exit
