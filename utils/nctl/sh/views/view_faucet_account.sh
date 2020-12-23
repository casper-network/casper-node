#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/views/funcs.sh

log "on-chain faucet account details:"
render_account $NCTL_ACCOUNT_TYPE_FAUCET
