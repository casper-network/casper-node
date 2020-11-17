#!/usr/bin/env bash

set -o errexit
set -o nounset
set -o pipefail

generate_timestamp() {
    local SCRIPT=(
        "from datetime import datetime, timedelta;"
        "print((datetime.utcnow() + timedelta(seconds=40)).isoformat('T') + 'Z')"
    )

    python3 -c "${SCRIPT[*]}"
}

generate_chainspec() {
    local BASEDIR=${1}
    local TIMESTAMP=${2}
    local SOURCE="${BASEDIR}/resources/local/chainspec.toml.in"
    local TARGET="${BASEDIR}/resources/local/chainspec.toml"

    export BASEDIR
    export TIMESTAMP

    echo "Generating chainspec..."
    envsubst < ${SOURCE} > ${TARGET}
}

main() {
    local BASEDIR="$(readlink -f $(dirname ${0}))"
    local TIMESTAMP="$(generate_timestamp)"

    generate_chainspec ${BASEDIR} ${TIMESTAMP}
}

main
