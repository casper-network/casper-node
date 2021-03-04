#!/usr/bin/env bash

# A type of actor representing a participating node.
export NCTL_ACCOUNT_TYPE_FAUCET="faucet"

# A type of actor representing a participating node.
export NCTL_ACCOUNT_TYPE_NODE="node"

# A type of actor representing a user.
export NCTL_ACCOUNT_TYPE_USER="user"

# Base RPC server port number.
export NCTL_BASE_PORT_RPC=40000

# Base JSON server port number.
export NCTL_BASE_PORT_REST=50000

# Base event server port number.
export NCTL_BASE_PORT_SSE=60000

# Base network server port number.
export NCTL_BASE_PORT_NETWORK=34452

# Set of chain system contracts.
export NCTL_CONTRACTS_CLIENT=(
    add_bid.wasm
    delegate.wasm
    transfer_to_account_u512.wasm
    transfer_to_account_u512_stored.wasm
    undelegate.wasm
    withdraw_bid.wasm
)

# Default amount used when delegating.
export NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT=1000000000   # (1e9)

# Default motes to pay for consumed gas.
export NCTL_DEFAULT_GAS_PAYMENT=10000000000   # (1e10)

# Default gas price multiplier.
export NCTL_DEFAULT_GAS_PRICE=10

# Default amount used when making transfers.
export NCTL_DEFAULT_TRANSFER_AMOUNT=2500000000   # (1e9)

# Intitial balance of faucet account.
export NCTL_INITIAL_BALANCE_FAUCET=1000000000000000000000000000000000   # (1e33)

# Intitial balance of user account.
export NCTL_INITIAL_BALANCE_USER=1000000000000000000000000000000000   # (1e33)

# Intitial balance of validator account.
export NCTL_INITIAL_BALANCE_VALIDATOR=1000000000000000000000000000000000   # (1e33)

# Intitial delegation amount of a user account.
export NCTL_INITIAL_DELEGATION_AMOUNT=1000000000   # (1e9)

# Base weight applied to a validator at genesis.
export NCTL_VALIDATOR_BASE_WEIGHT=(
    1000000000000011
    1000000000000012
    1000000000000013
    1000000000000014
    1000000000000015
    1000000000000016
    1000000000000017
    1000000000000018
    1000000000000019
    1000000000000110
)

# Name of process group: boostrap validators.
export NCTL_PROCESS_GROUP_1=validators-1

# Name of process group: genesis validators.
export NCTL_PROCESS_GROUP_2=validators-2

# Name of process group: non-genesis validators.
export NCTL_PROCESS_GROUP_3=validators-3
