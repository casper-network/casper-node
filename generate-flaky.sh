#!/usr/bin/env bash

# This script generates chainspec, config

set -o errexit
set -o nounset
set -o pipefail

generate_timestamp() {
    local DELAY=${1}

    local SCRIPT=(
        "from datetime import datetime, timedelta, timezone;"
	"print((datetime.now(timezone.utc).replace(tzinfo=None) + timedelta(seconds=${DELAY})).isoformat('T') + 'Z')"
    )

    python3 -c "${SCRIPT[*]}"
}

generate_chainspec() {
    local BASEDIR=${1}
    local TIMESTAMP=${2}
    local SOURCE="${BASEDIR}/resources/local/chainspec.toml.in"
    local TARGET="${BASEDIR}/resources/flaky/chainspec.toml"

    export BASEDIR
    export TIMESTAMP
    
    touch "${BASEDIR}/resources/flaky/chainspec.toml"

    echo "Generating chainspec..."
    envsubst < ${SOURCE} > ${TARGET}
}

prepare_config() {
    local BASEDIR=${1}
    local SOURCE="${BASEDIR}/resources/local/config.toml"
    local TARGET="${BASEDIR}/resources/flaky/config.toml"
    cp ${SOURCE} ${TARGET}
    sed -i 's/# \[network.flakiness\]/[network.flakiness]/g' ${BASEDIR}/resources/flaky/config.toml
    sed -i 's/# drop_peer_after_min/drop_peer_after_min/g' ${BASEDIR}/resources/flaky/config.toml
    sed -i 's/# drop_peer_after_max/drop_peer_after_max/g' ${BASEDIR}/resources/flaky/config.toml
    sed -i 's/# block_peer_after_drop_min/block_peer_after_drop_min/g' ${BASEDIR}/resources/flaky/config.toml
    sed -i 's/# block_peer_after_drop_max/block_peer_after_drop_max/g' ${BASEDIR}/resources/flaky/config.toml
}

main() {
    local DELAY=${1:-40}
    local BASEDIR="$(readlink -f $(dirname ${0}))"
    local TIMESTAMP="$(generate_timestamp ${DELAY})"
    mkdir -p "${BASEDIR}/resources/flaky"
    generate_chainspec ${BASEDIR} ${TIMESTAMP}
    prepare_config ${BASEDIR}
}

main $@