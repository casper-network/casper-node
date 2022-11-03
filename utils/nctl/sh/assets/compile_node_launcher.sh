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

pushd "$NCTL_CASPER_NODE_LAUNCHER_HOME" || \
    { echo "Could not find the casper-node-launcher repo - have you cloned it into your working directory?"; exit; }

unset OPTIND #clean OPTIND envvar, otherwise getopts can break.
COMPILE_MODE="release" #default compile mode to release.

while getopts 'd' opt; do 
    case $opt in
        d ) 
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
