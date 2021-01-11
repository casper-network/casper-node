#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh

log "on-chain faucet account details:"
render_account "$NCTL_ACCOUNT_TYPE_FAUCET"
