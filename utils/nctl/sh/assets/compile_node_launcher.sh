#!/usr/bin/env bash
#
#######################################
# Compiles node launcher software.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_NODE_LAUNCHER_HOME - path to casper node launcher repo.
#   NCTL_COMPILE_TARGET - flag indicating whether software compilation target is release | debug.
########################################

# Import utils.
source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_NODE_LAUNCHER_HOME" || exit

if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build
else
    cargo build --release
fi

popd || exit
