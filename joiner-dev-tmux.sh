#!/usr/bin/env bash
# -*- mode: sh; fill-column: 80; sh-basic-offset: 4; -*-

# This file is formatted with `shfmt -i 4 -ci`
# and linted with shellcheck

set -o errexit
set -o nounset
set -o pipefail

tmux_new_window() {
    local SESSION=${1}
    local ID=${2}
    local CMD=${3}
    local NAME="${SESSION}-${ID}"

    tmux new-window -t "${SESSION}:${ID}" -n "${NAME}"
    tmux send-keys -t "${NAME}" "${CMD}" C-m
}

build_system_contracts() {
    local CMD=(
        "make -s"
        "build-contracts-rs"
        "CARGO_FLAGS=--quiet"
    )

    echo "Building system contracts..."
    ${CMD[*]}
}

build_node() {
    local CMD=(
        "cargo build"
        "--quiet"
        "--manifest-path=node/Cargo.toml"
    )

    echo "Building node..."
    ${CMD[*]}
}

build_casper_client() {
    local CMD=(
        "cargo build"
        "--quiet"
        "-p casper-client"
    )

    echo "Building casper-client..."
    ${CMD[*]}
}

generate_timestamp() {
    local DELAY=${1}

    local SCRIPT=(
        "from datetime import datetime, timedelta;"
        "print((datetime.utcnow() + timedelta(seconds=${DELAY})).isoformat('T') + 'Z')"
    )

    python3 -c "${SCRIPT[*]}"
}

generate_chainspec() {
    local BASEDIR=${1}
    local TIMESTAMP=${2}
    local SOURCE="${BASEDIR}/resources/joiner/chainspec.toml.in"
    local TARGET="${BASEDIR}/resources/joiner/chainspec.toml"

    export BASEDIR
    export TIMESTAMP

    echo "Generating chainspec..."
    envsubst <"${SOURCE}" >"${TARGET}"
}

run_node() {
    local EXECUTABLE=${1}
    local SESSION=${2}
    local ID=${3}
    local CONFIG_DIR=${4}
    local DATA_DIR=${5}
    local BOOTSTRAP_RPC_PORT=${6}

    local CONFIG_TOML_PATH="${CONFIG_DIR}/config.toml"
    local SECRET_KEY_PATH="${CONFIG_DIR}/secret_keys/node-${ID}.pem"
    local STORAGE_DIR="${DATA_DIR}/node-${ID}-storage"

    local CMD=(
        "${EXECUTABLE}"
        "validator"
        "${CONFIG_TOML_PATH}"
        "-C consensus.secret_key_path=${SECRET_KEY_PATH}"
        "-C storage.path=${STORAGE_DIR}"
        "-C rpc_server.address='0.0.0.0:${BOOTSTRAP_RPC_PORT}'"
        "1> >(tee ${DATA_DIR}/node-${ID}.log) 2> >(tee ${DATA_DIR}/node-${ID}.log.stderr)"
    )

    mkdir -p "${STORAGE_DIR}"
    tmux_new_window "${SESSION}" "${ID}" "${CMD[*]}"
    echo "Booting node ${ID}..."
}

run_joiner_node() {
    local EXECUTABLE=${1}
    local SESSION=${2}
    local ID=${3}
    local CONFIG_DIR=${4}
    local DATA_DIR=${5}
    local BOOTSTRAP_RPC_PORT=${6}

    local RPC_URL="http://127.0.0.1:${BOOTSTRAP_RPC_PORT}"
    local GET_LATEST_BLOCK_HASH_CMD=(
        "cargo"
        "run"
        "--quiet"
        "-p" "casper-client"
        "--"
        "get-block"
        "--node-address=${RPC_URL}"

        "|"

        "jq"
        ".result.block.hash"
    )

    local GET_TRUSTED_HASH=(
        "TRUSTED_HASH='null';"
        "while [[ \${TRUSTED_HASH} == 'null' ]] ; do"
        "TRUSTED_HASH=\$(${GET_LATEST_BLOCK_HASH_CMD[*]});"
        "echo \${TRUSTED_HASH};"
        "sleep 5;"
        "done"
        "1> >(tee ${DATA_DIR}/node-${ID}.log) 2> >(tee ${DATA_DIR}/node-${ID}.log.stderr)"
    )

    local CONFIG_TOML_PATH="${CONFIG_DIR}/config.toml"
    local SECRET_KEY_PATH="${CONFIG_DIR}/secret_keys/node-${ID}.pem"
    local STORAGE_DIR="${DATA_DIR}/node-${ID}-storage"
    local RUN_JOINER=(
        "${EXECUTABLE}"
        "validator"
        "${CONFIG_TOML_PATH}"
        "-C consensus.secret_key_path=${SECRET_KEY_PATH}"
        "-C storage.path=${STORAGE_DIR}"
        "-C network.bind_address='0.0.0.0:0'"
        "-C rpc_server.address='0.0.0.0:0'"
        "-C rest_server.address='0.0.0.0:0'"
        "-C event_stream_server.address='0.0.0.0:0'"
        "-C node.trusted_hash=\"\${TRUSTED_HASH}\""
        "1> >(tee -a ${DATA_DIR}/node-${ID}.log) 2> >(tee -a ${DATA_DIR}/node-${ID}.log.stderr)"
    )

    local CMD=(
        "${GET_TRUSTED_HASH[*]}"
        "&&"
        "${RUN_JOINER[*]}"
    )

    tmux_new_window "${SESSION}" "${ID}" "${CMD[*]}"
    echo "Launching joiner node ${ID}..."
}

check_for_bootstrap() {
    local BOOTSTRAP_PORT=34553

    while ! (: </dev/tcp/0.0.0.0/${BOOTSTRAP_PORT}) &>/dev/null; do
        sleep 1
    done
}

main() {
    local BASEDIR
    BASEDIR="$(readlink -f "$(dirname "${0}")")"
    local CONFIG_DIR="${BASEDIR}/resources/joiner"
    local EXECUTABLE="${BASEDIR}/target/debug/casper-node"

    local DELAY=${1:-40}
    local TIMESTAMP
    TIMESTAMP="$(generate_timestamp "${DELAY}")"

    local BOOTSTRAP_RPC_PORT="${BOOTSTRAP_RPC_PORT:-50101}"

    export RUST_LOG="${RUST_LOG:-debug}"
    export TMPDIR="${TMPDIR:-$(mktemp -d)}"

    # create a new tmux session
    # if one already exists this will fail and the program will exit
    local SESSION="${SESSION:-local}"
    tmux new-session -d -s "${SESSION}"

    if [[ ! -x "${EXECUTABLE}" || ! -v QUICK_START ]]; then
        build_system_contracts

        build_node

        build_casper_client
    fi

    generate_chainspec "${BASEDIR}" "${TIMESTAMP}"

    local ID=1
    run_node "${EXECUTABLE}" "${SESSION}" "${ID}" "${CONFIG_DIR}" "${TMPDIR}" "${BOOTSTRAP_RPC_PORT}"
    check_for_bootstrap

    local ID=2
    run_joiner_node "${EXECUTABLE}" "${SESSION}" "${ID}" "${CONFIG_DIR}" "${TMPDIR}" "${BOOTSTRAP_RPC_PORT}"

    echo
    echo "DELAY     : ${DELAY}"
    echo "RPC       : http://127.0.0.1:${BOOTSTRAP_RPC_PORT}"
    echo "RUST_LOG  : ${RUST_LOG}"
    echo "TIMESTAMP : ${TIMESTAMP}"
    echo "TMPDIR    : ${TMPDIR}"
    echo
    echo "To view: "
    echo "    tmux attach-session -t ${SESSION}"
    echo
    echo "To kill: "
    echo "    tmux kill-session -t ${SESSION}"
    echo
}

main "${@}"
