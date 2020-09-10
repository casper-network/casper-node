#!/bin/sh
#
# run-dev: A quick and dirty script to run a testing setup of local nodes.

set -eu

BASEDIR=$(readlink -f $(dirname $0))
CHAINSPEC=/tmp/chainspec.toml

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
    else
        BIND_ADDRESS_ARG=
    fi

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
        --config-ext=consensus.secret_key_path=secret_keys/node-${ID}.pem \
        --config-ext=storage.path=${STORAGE_DIR} \
        --config-ext=network.gossip_interval=1000 \
        --config-ext=node.chainspec_config_path=${CHAINSPEC} \
        ${BIND_ADDRESS_ARG}

    echo "Started node $ID, logfile: ${LOGFILE}"

    # Sleep so that nodes are actually started in sequence.
    # Hopefully, fixes some of the race condition issues during startup.
    sleep 1;
}

# Build the node first, so that `sleep` in the loop has an effect.
cargo build -p casper-node

# Update the chainspec to use the current time as the genesis timestamp.
cp ${BASEDIR}/resources/local/chainspec.toml ${CHAINSPEC}
sed -i "s/timestamp = [[:digit:]]*/timestamp = $(date '+%s000')/" ${CHAINSPEC}
sed -i 's|\.\./\.\.|'"$BASEDIR"'|' ${CHAINSPEC}
sed -i 's|accounts\.csv|'"$BASEDIR"'/resources/local/accounts.csv|' ${CHAINSPEC}

for i in 1 2 3 4 5; do
    run_node $i
done;

echo "Test network started."
echo
echo "To stop all nodes, run"
echo "  systemctl --user stop node-\\*"
echo "  systemctl --user reset-failed"
