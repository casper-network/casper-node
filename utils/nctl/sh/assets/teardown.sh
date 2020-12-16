#!/usr/bin/env bash

source $NCTL/sh/utils.sh

#######################################
# Destructure input args.
#######################################

unset NET_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        *)
    esac
done

NET_ID=${NET_ID:-1}

#######################################
# Main
#######################################

log "net-$NET_ID: tearing down assets ... please wait"

# Stop all spinning nodes.
source $NCTL/sh/node/stop.sh net=$NET_ID node=all

# If supervisord - kill.
if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
    do_supervisord_kill $NET_ID
fi

# Delete artefacts.
rm -rf $NCTL/assets/net-$NET_ID

log "net-$NET_ID: assets torn down."
