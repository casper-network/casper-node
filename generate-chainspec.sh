#!/usr/bin/env bash

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
    local TARGET="${BASEDIR}/resources/local/chainspec.toml"

    export BASEDIR
    export TIMESTAMP

    echo "Generating chainspec..."
    envsubst < ${SOURCE} > ${TARGET}

    # Keep the local protocol package self-contained, including runtime attribution.
    cp -R "${BASEDIR}/resources/evm/preinstalls" "${BASEDIR}/resources/local/"
}

main() {
    local DELAY=${1:-40}
    local BASEDIR="$(readlink -f $(dirname ${0}))"
    local TIMESTAMP="$(generate_timestamp ${DELAY})"

    generate_chainspec ${BASEDIR} ${TIMESTAMP}
}

main $@
