#!/usr/bin/env bash

set -eu -o pipefail

SERVER=casperlabs-engine-grpc-server
CLIENT=concurrent-executor
EE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." >/dev/null 2>&1 && pwd)"
THIS_SCRIPT="$(basename ${BASH_SOURCE[0]})"
TEMP_DIR=$(mktemp -d -t ${CLIENT}-XXXXX)
DATA_DIR="${TEMP_DIR}/DataDir"
SOCKET="${TEMP_DIR}/Socket"
MIN_THREADS=1
MAX_THREADS=100
MIN_MESSAGES=1
MAX_MESSAGES=1000000

print_usage_and_exit() {
    printf "USAGE:\n"
    printf "    %s <SERVER-THREAD-COUNT> <CLIENT-THREAD-COUNT> <MESSAGE-COUNT> [FLAGS]\n\n" "$THIS_SCRIPT"
    printf "ARGS:\n"
    printf "    <SERVER-THREAD-COUNT>   Number of threads for the server threadpool [%d-%d]\n" $MIN_THREADS $MAX_THREADS
    printf "    <CLIENT-THREAD-COUNT>   Number of threads for the client threadpool [%d-%d]\n" $MIN_THREADS $MAX_THREADS
    printf "    <MESSAGE-COUNT>         Total number of messages the client should send [%d-%d]\n\n" $MIN_MESSAGES $MAX_MESSAGES
    printf "FLAGS:\n"
    printf "    -z   Use system contracts instead of host-side logic for Mint, Proof of Stake and Standard Payment\n\n"
    exit 127
}

validate_and_assign_arg() {
    local ARG=$1
    local MIN=$2
    local MAX=$3
    local -n OUT_VAR=$4
    [[ $1 != *[^0-9]* && $1 -ge $MIN && $1 -le $MAX ]] || print_usage_and_exit
    OUT_VAR=$ARG
}

validate_and_assign_flag() {
    local ARG=$1
    local -n OUT_VAR=$2
    [[ $1 == "-z" ]] || print_usage_and_exit
    OUT_VAR=$ARG
}

build_contracts_and_run_state_initializer() {
    cd $EE_DIR
    make build-contracts-rs
    cd engine-tests/
    PRE_STATE_HASH=$(cargo run --release --bin=state-initializer -- --data-dir=$DATA_DIR)
}

run_server() {
    cd $EE_DIR
    cargo build --release --bin=$SERVER
    target/release/$SERVER $SOCKET --data-dir=$DATA_DIR --threads=$SERVER_THREAD_COUNT $USE_SYSTEM_CONTRACTS &
    SERVER_PID=$!
}

run_client() {
    cd ${EE_DIR}/engine-tests/
    CLIENT_OUTPUT=$(cargo run --release --bin=$CLIENT -- \
        --socket=$SOCKET --pre-state-hash=$PRE_STATE_HASH --threads=$CLIENT_THREAD_COUNT --requests=$MESSAGE_COUNT)
}

kill_server() {
    kill -SIGINT $SERVER_PID
    wait $SERVER_PID
}

clean_up() {
    rm -rf $TEMP_DIR
}

trap clean_up EXIT

if [[ $# -eq 3 ]]; then
    validate_and_assign_arg $1 $MIN_THREADS  $MAX_THREADS  SERVER_THREAD_COUNT
    validate_and_assign_arg $2 $MIN_THREADS  $MAX_THREADS  CLIENT_THREAD_COUNT
    validate_and_assign_arg $3 $MIN_MESSAGES $MAX_MESSAGES MESSAGE_COUNT
    USE_SYSTEM_CONTRACTS=
elif [[ $# -eq 4 ]]; then
    validate_and_assign_arg $1 $MIN_THREADS  $MAX_THREADS  SERVER_THREAD_COUNT
    validate_and_assign_arg $2 $MIN_THREADS  $MAX_THREADS  CLIENT_THREAD_COUNT
    validate_and_assign_arg $3 $MIN_MESSAGES $MAX_MESSAGES MESSAGE_COUNT
    validate_and_assign_flag $4 USE_SYSTEM_CONTRACTS
else
    print_usage_and_exit
fi

build_contracts_and_run_state_initializer
run_server
run_client
kill_server

printf "\nWith %d Server threads and %d Client threads, %s\n\n" $SERVER_THREAD_COUNT $CLIENT_THREAD_COUNT "$CLIENT_OUTPUT"
