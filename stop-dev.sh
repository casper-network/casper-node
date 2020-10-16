#!/bin/sh
#
# stop-dev: A quick and dirty script to stop a testing setup of local nodes.

set -eu

ARGS="$@"
# If no nodes defined, stop all.
NODES="${ARGS:-1 2 3 4 5}"

# print the warning if node 1 is one of the selected nodes to be stopped
case "$NODES" in
    *"1"*) echo "NOTE: Stopping node 1 will also stop other nodes started with run-dev.sh"
esac

for i in $NODES; do
    systemctl --user stop node-$i
done;

rm /tmp/chainspec_*
