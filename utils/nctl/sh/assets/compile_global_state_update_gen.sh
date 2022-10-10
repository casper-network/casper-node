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

unset OPTIND #clean OPTIND envvar, otherwise getopts can break.
COMPILE_MODE="release" #default compile mode to release.

while getopts 'd' opt; do 
    case $opt in
        d ) 
            echo "-d global_state"
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