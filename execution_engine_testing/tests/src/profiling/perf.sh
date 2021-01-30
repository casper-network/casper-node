#!/usr/bin/env bash

set -eu -o pipefail

RED='\033[0;31m'
CYAN='\033[0;36m'
NO_COLOR='\033[0m'
EE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." >/dev/null 2>&1 && pwd)"
TEMP_DIR=$(mktemp -d -t simple-transfer-perf-XXXXX)

check_for_perf() {
    if ! [[ -x "$(command -v perf)" ]]; then
        printf "${RED}perf not installed${NO_COLOR}\n\n"
        printf "For Debian, try:\n"
        printf "${CYAN}sudo apt install linux-tools-common linux-tools-generic linux-tools-$(uname -r)${NO_COLOR}\n\n"
        printf "For Redhat, try:\n"
        printf "${CYAN}sudo yum install perf${NO_COLOR}\n\n"
        exit 127
    fi
}

run_perf() {
    cd $EE_DIR
    make build-contracts
    cd engine-tests/
    cargo build --release --bin state-initializer
    cargo build --release --bin simple-transfer
    TARGET_DIR="${EE_DIR}/target/release"
    DATA_DIR_ARG="--data-dir=../target"
    ${TARGET_DIR}/state-initializer ${DATA_DIR_ARG} | perf record -g --call-graph dwarf ${TARGET_DIR}/simple-transfer ${DATA_DIR_ARG}
    mv perf.data ${TEMP_DIR}
}

check_or_clone_flamegraph() {
    FLAMEGRAPH_DIR="/tmp/FlameGraph"
    export PATH=${FLAMEGRAPH_DIR}:${PATH}
    if ! [[ -x "$(command -v stackcollapse-perf.pl)" ]] || ! [[ -x "$(command -v flamegraph.pl)" ]]; then
        rm -rf ${FLAMEGRAPH_DIR}
        git clone --depth=1 https://github.com/brendangregg/FlameGraph ${FLAMEGRAPH_DIR}
    fi
}

create_flamegraph() {
    FLAMEGRAPH="${TEMP_DIR}/flame.svg"
    printf "Creating flamegraph at ${FLAMEGRAPH}\n"
    cd ${TEMP_DIR}
    perf script | stackcollapse-perf.pl | flamegraph.pl > ${FLAMEGRAPH}
    x-www-browser ${FLAMEGRAPH}
}

check_for_perf
run_perf
check_or_clone_flamegraph
create_flamegraph
