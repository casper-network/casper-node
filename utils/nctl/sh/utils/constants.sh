#!/usr/bin/env bash

# A type of actor representing a participating node.
export NCTL_ACCOUNT_TYPE_FAUCET="faucet"

# A type of actor representing a participating node.
export NCTL_ACCOUNT_TYPE_NODE="node"

# A type of actor representing a user.
export NCTL_ACCOUNT_TYPE_USER="user"

# Base RPC server port number.
export NCTL_BASE_PORT_RPC=11000

# Base JSON server port number.
export NCTL_BASE_PORT_REST=14000

# Base event server port number.
export NCTL_BASE_PORT_SSE=18000

# Base network server port number.
export NCTL_BASE_PORT_NETWORK=22000

# Set of client side auction contracts.
export NCTL_CONTRACTS_CLIENT_AUCTION=(
    "activate_bid.wasm"
    "add_bid.wasm"
    "delegate.wasm"
    "undelegate.wasm"
    "withdraw_bid.wasm"
)

# Set of client side shared contracts.
export NCTL_CONTRACTS_CLIENT_SHARED=(
    "named_purse_payment.wasm"
)

# Set of client side transfer contracts.
export NCTL_CONTRACTS_CLIENT_TRANSFERS=(
    "transfer_to_account_u512.wasm"
)

# Default amount used when delegating.
export NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT=1000000000   # (1e9)

# Default era offset to apply when activating an upgrade.
export NCTL_DEFAULT_ERA_ACTIVATION_OFFSET=2

# Default motes to pay for consumed gas.
export NCTL_DEFAULT_GAS_PAYMENT=100000000000   # (1e11)

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
export NCTL_INITIAL_DELEGATION_AMOUNT=1000000000000000   # (1e15)

# Base weight applied to a validator at genesis.
export NCTL_VALIDATOR_BASE_WEIGHT=1000000000000000   # (1e15)

# Name of process group: boostrap validators.
export NCTL_PROCESS_GROUP_1=validators-1

# Name of process group: genesis validators.
export NCTL_PROCESS_GROUP_2=validators-2

# Name of process group: non-genesis validators.
export NCTL_PROCESS_GROUP_3=validators-3
