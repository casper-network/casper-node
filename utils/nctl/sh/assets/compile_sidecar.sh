#!/usr/bin/env bash
#
#######################################
# Compiles the sidecar.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_HOME - path to casper node repo.
#   NCTL_CASPER_SIDECAR_HOME - path to casper sidecar repo.
#   NCTL_COMPILE_TARGET - flag indicating whether software compilation target is release | debug.
########################################

# Import utils.
source "$NCTL"/sh/utils/main.sh

unset OPTIND #clean OPTIND envvar, otherwise getopts can break.
COMPILE_MODE="release" #default compile mode to release.

# Build client utility.
pushd "$NCTL_CASPER_SIDECAR_HOME" || \
    { echo "Could not find the casper sidecar repo - have you cloned it into your working directory?"; exit; }

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
