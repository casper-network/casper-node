#!/usr/bin/env bash

#######################################
# Returns an on-chain account balance.
# Arguments:
#   Data to be hashed.
#######################################
function get_account_balance()
{
    local PURSE_UREF=${1}
    local STATE_ROOT_HASH=${2:-$(get_state_root_hash)}
    local ACCOUNT_BALANCE
    local NODE_ADDRESS

    NODE_ADDRESS=$(get_node_address_rpc)
    ACCOUNT_BALANCE=$(
        $(get_path_to_client) query-balance \
            --node-address "$NODE_ADDRESS" \
            --state-root-hash "$STATE_ROOT_HASH" \
            --purse-uref "$PURSE_UREF" \
            | jq '.result.balance' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    echo "$ACCOUNT_BALANCE"
}

#######################################
# Returns an on-chain account hash.
# Arguments:
#   Data to be hashed.
#######################################
function get_account_hash()
{
    local ACCOUNT_KEY=${1}
    local ACCOUNT_PBK=${ACCOUNT_KEY:2}

    local SCRIPT=(
        "import hashlib;"
        "as_bytes=bytes('ed25519', 'utf-8') + bytearray(1) + bytes.fromhex('$ACCOUNT_PBK');"
        "h=hashlib.blake2b(digest_size=32);"
        "h.update(as_bytes);"
        "print(h.digest().hex());"
     )

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Returns an account key.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function get_account_key()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}

    if [ "$ACCOUNT_TYPE" = "$NCTL_ACCOUNT_TYPE_FAUCET" ]; then
        cat "$(get_path_to_faucet)"/public_key_hex
    elif [ "$ACCOUNT_TYPE" = "$NCTL_ACCOUNT_TYPE_NODE" ]; then
        cat "$(get_path_to_node "$ACCOUNT_IDX")"/keys/public_key_hex
    elif [ "$ACCOUNT_TYPE" = "$NCTL_ACCOUNT_TYPE_USER" ]; then
        cat "$(get_path_to_user "$ACCOUNT_IDX")"/public_key_hex
    fi
}

#######################################
# Returns an account prefix used when logging.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
# Arguments:
#   Account type.
#   Account index (optional).
#######################################
function get_account_prefix()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2:-}
    local NET_ID=${NET_ID:-1}

    local PREFIX="net-$NET_ID.$ACCOUNT_TYPE"
    if [ "$ACCOUNT_TYPE" != "$NCTL_ACCOUNT_TYPE_FAUCET" ]; then
        PREFIX=$PREFIX"-"$ACCOUNT_IDX
    fi

    echo "$PREFIX"
}

#######################################
# Returns a main purse uref.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Account key.
#   State root hash.
#######################################
function get_main_purse_uref()
{
    local ACCOUNT_KEY=${1}
    local STATE_ROOT_HASH=${2:-$(get_state_root_hash)}

    source "$NCTL"/sh/views/view_chain_account.sh \
        account-key="$ACCOUNT_KEY" \
        root-hash="$STATE_ROOT_HASH" \
        | jq '.stored_value.Account.main_purse' \
        | sed -e 's/^"//' -e 's/"$//'
}
