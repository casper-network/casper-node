#!/bin/sh
#
# run-dev: A quick and dirty script to run a testing setup of local nodes.

set -eu

BASEDIR=$(readlink -f $(dirname $0))

run_node() {
    ID=$1
    STORAGE_DIR=/tmp/node-${ID}-storage
    LOGFILE=/tmp/node-${ID}.log
    rm -rf ${STORAGE_DIR}
    rm -f ${LOGFILE}
    mkdir -p ${STORAGE_DIR}

    if [ $1 -ne 1 ]
    then
        BIND_ADDRESS_ARG=--config-ext=network.bind_address='0.0.0.0:0'
        DEPS="--property=After=node-1.service --property=Requires=node-1.service"
    else
        BIND_ADDRESS_ARG=
        DEPS=
    fi

    # We run with a 10 minute timeout, to allow for compilation and loading.
    systemd-run \
        --user \
        --unit node-$ID \
        --description "Casper Dev Node ${ID}" \
        --collect \
        --no-block \
        --property=Type=notify \
        --property=TimeoutSec=600 \
        --property=WorkingDirectory=${BASEDIR} \
        $DEPS \
        --setenv=RUST_LOG=debug \
        --property=StandardOutput=file:${LOGFILE} \
        --property=StandardError=file:${LOGFILE}.stderr \
        -- \
        cargo run -p casper-node \
        validator \
        resources/local/config.toml \
        --config-ext=network.systemd_support=true \
        --config-ext=consensus.secret_key_path=secret_keys/node-${ID}.pem \
        --config-ext=storage.path=${STORAGE_DIR} \
        --config-ext=network.gossip_interval=1000 \
        ${BIND_ADDRESS_ARG}

    echo "Started node $ID, logfile: ${LOGFILE}"

    # Sleep so that nodes are actually started in sequence.
    # Hopefully, fixes some of the race condition issues during startup.
    sleep 1;
}

# Build the node first, so that `sleep` in the loop has an effect.
cargo build -p casper-node

for i in 1 2 3 4 5; do
    run_node $i
done;

echo "Test network starting."
echo
echo "To stop all nodes, run"
echo "  systemctl --user stop node-\\*"
echo "  systemctl --user reset-failed"
