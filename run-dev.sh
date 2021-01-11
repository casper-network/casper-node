#!/bin/sh
#
# run-dev: A quick and dirty script to run a testing setup of local nodes.

set -eu

# Build the contracts
make build-contracts-rs

# Build the node first, so that `sleep` in the loop has an effect.
cargo build -p casper-node

BASEDIR=$(readlink -f $(dirname $0))
CHAINSPEC=$(mktemp -t chainspec_XXXXXXXX --suffix .toml)
TRUSTED_HASH="${TRUSTED_HASH:-}"

# Generate a genesis timestamp 30 seconds into the future, unless explicity given a different one.
TIMESTAMP=$(python3 -c 'from datetime import datetime, timedelta; print((datetime.utcnow() + timedelta(seconds=30)).isoformat("T") + "Z")')
TIMESTAMP=${GENESIS_TIMESTAMP:-$TIMESTAMP}

echo "GENESIS_TIMESTAMP=${TIMESTAMP}"

export BASEDIR
export TIMESTAMP

# Update the chainspec to use the current time as the genesis timestamp.
envsubst < ${BASEDIR}/resources/local/chainspec.toml.in > ${CHAINSPEC}

# If no nodes defined, start all.
NODES="${@:-1 2 3 4 5}"

run_node() {
    ID=$1
    STORAGE_DIR=/tmp/node-${ID}-storage
    LOGFILE=/tmp/node-${ID}.log
    rm -rf ${STORAGE_DIR}
    rm -f ${LOGFILE}
    rm -f ${LOGFILE}.stderr
    mkdir -p ${STORAGE_DIR}

    if [ $1 -ne 1 ]
    then
        BIND_ADDRESS_ARG=--config-ext=network.bind_address='0.0.0.0:0'
        RPC_SERVER_ADDRESS_ARG=--config-ext=rpc_server.address='0.0.0.0:0'
        REST_SERVER_ADDRESS_ARG=--config-ext=rest_server.address='0.0.0.0:0'
        EVENT_STREAM_SERVER_ADDRESS_ARG=--config-ext=event_stream_server.address='0.0.0.0:0'
        DEPS="--property=After=node-1.service --property=Requires=node-1.service"
    else
        BIND_ADDRESS_ARG=
        RPC_SERVER_ADDRESS_ARG=
        REST_SERVER_ADDRESS_ARG=
        EVENT_STREAM_SERVER_ADDRESS_ARG=
        DEPS=
    fi

    if ! [ -z "$TRUSTED_HASH" ]
    then
        TRUSTED_HASH_ARG=--config-ext=node.trusted_hash="${TRUSTED_HASH}"
    else
        TRUSTED_HASH_ARG=
    fi

    echo "$TRUSTED_HASH_ARG"

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
        --setenv=RUST_LOG=casper=trace \
        --property=StandardOutput=file:${LOGFILE} \
        --property=StandardError=file:${LOGFILE}.stderr \
        -- \
        cargo run -p casper-node -- \
        validator \
        resources/local/config.toml \
        --config-ext=network.systemd_support=true \
        --config-ext=consensus.secret_key_path=secret_keys/node-${ID}.pem \
        --config-ext=storage.path=${STORAGE_DIR} \
        --config-ext=network.gossip_interval=1000 \
        --config-ext=node.chainspec_config_path=${CHAINSPEC} \
        ${BIND_ADDRESS_ARG} \
        ${RPC_SERVER_ADDRESS_ARG} \
        ${REST_SERVER_ADDRESS_ARG} \
        ${EVENT_STREAM_SERVER_ADDRESS_ARG} \
        ${TRUSTED_HASH_ARG}

    echo "Started node $ID, logfile: ${LOGFILE}"

    # Sleep so that nodes are actually started in sequence.
    # Hopefully, fixes some of the race condition issues during startup.
    sleep 1;
}

for i in $NODES; do
    run_node $i
done;

echo "Test network starting."
echo
echo "To stop all nodes, run stop-dev.sh"
