#!/usr/bin/env bash
#
#######################################
# Compiles global-state-update-gen for emergency upgrades testing.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_HOME - path to casper node repo.
########################################

source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_HOME/utils/global-state-update-gen" || exit

cargo build --release

popd || exit
