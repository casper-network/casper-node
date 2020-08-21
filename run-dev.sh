#!/bin/sh
#
# run-dev: A quicky and dirty script to run a testing setup of local nodes.

set -eu

BASEDIR=$(readlink -f $(dirname $0))

run_node() {
    ID=$1
    STORAGE_DIR=/tmp/node-${ID}-storage
    rm -rf ${STORAGE_DIR}
    mkdir -p ${STORAGE_DIR}

    systemd-run \
        --user \
        --unit node-$ID \
        --description "CasperLabs Dev Node ${ID}" \
        --remain-after-exit \
        "--working-directory=${BASEDIR}" \
        --setenv=RUST_LOG=debug \
        --\
        cargo run -p casperlabs-node \
        validator \
        -c resources/local/config.toml \
        -C consensus.secret_key_path=secret_keys/node-${ID}.pem \
        -C storage.path=${STORAGE_DIR}
}

for i in 1 2 3 4 5; do
    run_node $i
done;

echo "Test network started."
echo
echo "To stop all nodes, run"
echo "  systemctl --user stop node-\\*"
echo "  systemctl --user reset-failed"
echo
echo "To see log output for a specific (node-n) or multiple (node-\\*) nodes, use"
echo "  journalctl --user -u node-\\* -f"
