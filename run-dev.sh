#!/bin/sh
#
# run-dev: A quicky and dirty script to run a testing setup of local nodes.

set -eu

BASEDIR=$(readlink -f $(dirname $0))

run_node() {
    ID=$1
    STORAGE_DIR=/tmp/node-${ID}-storage
    LOGFILE=/tmp/node-${ID}.log
    rm -rf ${STORAGE_DIR}
    rm -f ${LOGFILE}
    mkdir -p ${STORAGE_DIR}

    systemd-run \
        --user \
        --unit node-$ID \
        --description "Casper Dev Node ${ID}" \
        --collect \
        --property=WorkingDirectory=${BASEDIR} \
        --setenv=RUST_LOG=debug \
        --property=StandardOutput=file:${LOGFILE} \
        --property=StandardError=file:${LOGFILE}.stderr \
        -- \
        cargo run -p casper-node \
        validator \
        resources/local/config.toml \
        -C consensus.secret_key_path=secret_keys/node-${ID}.pem \
        -C storage.path=${STORAGE_DIR}

    echo "Started node $ID, logfile: ${LOGFILE}"
}

for i in 1 2 3 4 5; do
    run_node $i
done;

echo "Test network started."
echo
echo "To stop all nodes, run"
echo "  systemctl --user stop node-\\*"
echo "  systemctl --user reset-failed"
