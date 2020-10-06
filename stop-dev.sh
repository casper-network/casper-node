#!/bin/sh
#
# stop-dev: A quick and dirty script to stop a testing setup of local nodes.

set -eu

BASEDIR=$(readlink -f $(dirname $0))
CHAINSPEC=$(mktemp -t chainspec_XXXXXXXX --suffix .toml)

ARGS="$@"
# If no nodes defined, stop all.
NODES="${ARGS:-1 2 3 4 5}"

for i in $NODES; do
    case "$NODES" in
        *"$i"*) systemctl --user stop node-$i
    esac
done;

rm /tmp/chainspec_*
