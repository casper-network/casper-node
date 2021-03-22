#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh

ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")
ACCOUNT_HASH=$(get_account_hash "$ACCOUNT_KEY")
PATH_TO_ACCOUNT_SKEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")
STATE_ROOT_HASH=$(get_state_root_hash)
PURSE_UREF=$(get_main_purse_uref "$ACCOUNT_KEY" "$STATE_ROOT_HASH")
ACCOUNT_BALANCE=$(get_account_balance "$PURSE_UREF" "$STATE_ROOT_HASH")

log "faucet a/c secret key    : $PATH_TO_ACCOUNT_SKEY"
log "faucet a/c key           : $ACCOUNT_KEY"
log "faucet a/c hash          : $ACCOUNT_HASH"
log "faucet a/c purse         : $PURSE_UREF"
log "faucet a/c purse balance : $ACCOUNT_BALANCE"
log "faucet on-chain account  : see below"
render_account "$NCTL_ACCOUNT_TYPE_FAUCET"
