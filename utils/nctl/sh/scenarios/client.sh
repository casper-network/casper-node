#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration test that exercises
# the rust client binary.
#
#######################################

function main() {
    # DIRS
    local TEMP_DIR
    local KEYGEN_DIR
    local ED25519_DIR
    local SECP256K1_DIR
    local COMPLETION_DIR
    local CHAINSPEC_DIR
    local MAKE_TRANSFER_DIR
    local FAUCET_DIR
    local MAKE_DEPLOY_DIR

    TEMP_DIR='/tmp/client'
    KEYGEN_DIR="$TEMP_DIR/keygen"
    ED25519_DIR="$KEYGEN_DIR/Ed25519"
    SECP256K1_DIR="$KEYGEN_DIR/secp256k1"
    COMPLETION_DIR="$TEMP_DIR/generate-completion"
    CHAINSPEC_DIR="$TEMP_DIR/get-chainspec"
    MAKE_TRANSFER_DIR="$TEMP_DIR/make-transfer"
    FAUCET_DIR="$NCTL/assets/net-1/faucet"
    MAKE_DEPLOY_DIR="$TEMP_DIR/make-deploy"

    # HASHES - assigned later
    local DEPLOY_HASH
    local BLOCK_HASH

    # ACCOUNT HASHES - assigned later
    local ED25519_ACC_HASH
    local SECP256K1_ACC_HASH
    local FAUCET_ACC_HASH

    # FILES
    local DEPLOY_FILE_1
    local MAKE_DEPLOY_FILE
    local SIGNED_DEPLOY_FILE
    local FAUCET_HEX
    local FAUCET_PEM
    local ED25519_HEX
    local ED25519_PEM
    local SECP256K1_HEX
    local SECP256K1_PEM
    local DICT_WASM

    DEPLOY_FILE_1="$MAKE_TRANSFER_DIR/deploy_1.json"
    MAKE_DEPLOY_FILE="$MAKE_DEPLOY_DIR/make_deploy.json"
    SIGNED_DEPLOY_FILE="$MAKE_DEPLOY_DIR/signed_deploy.json"
    FAUCET_HEX="$FAUCET_DIR/public_key_hex"
    FAUCET_PEM="$FAUCET_DIR/public_key.pem"
    FAUCET_SECRET_KEY="$FAUCET_DIR/secret_key.pem"
    ED25519_HEX="$ED25519_DIR/public_key_hex"
    ED25519_PEM="$ED25519_DIR/public_key.pem"
    ED25519_SECRET_KEY="$ED25519_DIR/secret_key.pem"
    SECP256K1_HEX="$SECP256K1_DIR/public_key_hex"
    SECP256K1_PEM="SECP256K1_DIR/public_key.pem"
    DICT_WASM="$NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/nctl-dictionary.wasm"

    log "------------------------------------------------------------"
    log "Starting Scenario: client"
    log "------------------------------------------------------------"

    # Create the temp dir
    mkdir -p "$TEMP_DIR"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Check Subcommand Count
    compare_client_subcommand_count "$(get_client_subcommand_count)" "26"
    # 2. Test Each Subcommand Outputs Help
    test_subcommand_help
    # 3. Test generate-completion subcommand: Each SHELL
    test_generate_completion "$COMPLETION_DIR"
    # 4. Test list-rpcs subcommand
    test_list_rpcs
    # 5. Test get-chainspec subcommand
    test_get_chainspec "$CHAINSPEC_DIR"
    # 6. Test get-node-status subcommand
    test_get_node_status
    # 7. Test get-peers subcommand
    test_get_peers
    # 8. Test get-validator-changes subcommand
    test_get_validator_changes
    # 9. Test get-auction-info subcommand
    test_get_auction_info
    # 10. Test get-state-root-hash subcommand
    test_get_state_root_hash
    # 11. Test get-block subcommand
    test_get_block
    # 12. Test keygen subcommand: Each ALGO
    test_keygen "$KEYGEN_DIR"
    # 13. Test account-address subcommand: ED25519
    test_account_address "$ED25519_DIR"
    # 14. Test account-address subcommand: secp256k1
    test_account_address "$SECP256K1_DIR"

    # Reused account hashes below
    ED25519_ACC_HASH=$(get_account_hash "$ED25519_PEM")
    SECP256K1_ACC_HASH=$(get_account_hash "$SECP256K1_HEX")
    FAUCET_ACC_HASH=$(get_account_hash "$FAUCET_HEX")

    # 15. Test make-transfer subcommand: Ed25519 PEM
    test_make_transfer "$ED25519_ACC_HASH" \
        "$MAKE_TRANSFER_DIR" \
        "$FAUCET_DIR" \
        "1"
    # 16. Test make-transfer subcommand: secp256k1 HEX
    test_make_transfer "$SECP256K1_ACC_HASH" \
        "$MAKE_TRANSFER_DIR" \
        "$FAUCET_DIR" \
        "2"

    # Get Deploy Hash for reuse
    DEPLOY_HASH=$(get_deploy_hash_from_file "$DEPLOY_FILE_1")

    # 17. Test send-deploy subcommand
    test_send_deploy "$DEPLOY_FILE_1"
    # 18. Test get-deploy subcommand
    test_get_deploy "$DEPLOY_HASH"

    # Get Block Hash for reuse
    BLOCK_HASH=$(get_block_containing_deploy_hash "$DEPLOY_HASH")

    # 19. Test get-block subcommand
    test_get_block_by_hash "$BLOCK_HASH"
    # 20. Test get-block-transfers subcommand
    test_get_block_transfers "$BLOCK_HASH" "$DEPLOY_HASH" "$FAUCET_ACC_HASH" "$ED25519_ACC_HASH"
    # 21. Test list-deploys subcommand
    test_list_deploys "$BLOCK_HASH" "$DEPLOY_HASH"
    # 22. Test get-era-info subcommand
    test_get_era_info "$BLOCK_HASH"
    # 23. Test query-global-state subcommand
    test_query_global_state "$BLOCK_HASH" "$DEPLOY_HASH"
    # 24. Test query-balance subcommand
    test_query_balance "$BLOCK_HASH" "$ED25519_HEX" "2500000000"
    # 25. Test get-account subcommand
    test_get_account "$ED25519_HEX" "$ED25519_ACC_HASH"
    # 26. Test transfer subcommand
    test_transfer "$ED25519_ACC_HASH" "$FAUCET_DIR" '3'
    # 27. Test make-deploy subcommand
    test_make_deploy "$ED25519_ACC_HASH" "$MAKE_DEPLOY_DIR" "$FAUCET_DIR"
    # 28. Test sign-deploy subcommand
    test_sign_deploy "$MAKE_DEPLOY_FILE" "$SIGNED_DEPLOY_FILE" "$ED25519_SECRET_KEY"
    # 29. Test send-deploy subcommand - retest: make/sign deploy
    test_send_deploy "$SIGNED_DEPLOY_FILE"
    # 30. Test put-deploy subcommand
    test_put_deploy "$DICT_WASM" "$FAUCET_SECRET_KEY"
    # 31. Test get-dictionary-item subcommand
    test_get_dictionary_item "$FAUCET_ACC_HASH"
    # 32. Test get-balance subcommand
    test_get_balance "$ED25519_HEX" "7500000000"
    # 33. Cleanup
    clean_tmp_dir

    log "------------------------------------------------------------"
    log "Scenario client complete"
    log "------------------------------------------------------------"
}

# casper-client get-balance
# ... gets balance from purse uref
# ... checks valid json with jq
# ... checks expected balance
function test_get_balance() {
    local PUBLIC_KEY=${1}
    local BALANCE=${2}
    local PURSE_UREF
    local OUTPUT

    log_step "Testing Client Subcommand: get-balance"

    PURSE_UREF=$(get_uref_for_public_key "$PUBLIC_KEY")

    echo "$PURSE_UREF"

    OUTPUT=$($(get_path_to_client) get-balance \
        --node-address "$(get_node_address_rpc)" \
        --state-root-hash "$(get_state_root_hash)" \
        --purse-uref "$PURSE_UREF")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Balance Check
    if [ "$BALANCE" = "$(echo $OUTPUT | jq -r '.result.balance_value')" ]; then
        log "... balance match! [expected]"
    else
        log "ERROR: Mismatched balance!"
        exit 1
    fi
}

# casper-client get-dictionary-item
# ... use contract from test_put_deploy
# ... checks valid json with jq
function test_get_dictionary_item() {
    local ACCOUNT_HASH=${1}
    local STATE_ROOT_HASH
    local DICTIONARY_KEY
    local DICTIONARY_NAME
    local OUTPUT

    log_step "Testing Client Subcommand: get-dictionary-item"

    DICTIONARY_KEY="foo"
    DICTIONARY_NAME="nctl_dictionary"

    OUTPUT=$($(get_path_to_client) get-dictionary-item \
        --node-address "$(get_node_address_rpc)" \
        --state-root-hash "$(get_state_root_hash)" \
        --dictionary-item-key "$DICTIONARY_KEY" \
        --dictionary-name "$DICTIONARY_NAME" \
        --account-hash "$ACCOUNT_HASH")

    # Check client responded
    test_with_jq "$OUTPUT"
}

# casper-client sign-deploy
# ... signs deploy
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_sign_deploy() {
    local INPUT_FILE=${1}
    local OUTPUT_FILE=${2}
    local SIGNER_KEY=${3}
    local OUTPUT
    local SIGNER_1
    local SIGNER_2

    log_step "Testing Client Subcommand: sign-deploy"

    OUTPUT=$($(get_path_to_client) sign-deploy \
        --input "$INPUT_FILE" \
        --secret-key "$SIGNER_KEY")

    # Test Valid JSON
    echo "$OUTPUT" | jq '.'

    SIGNER_1=$(echo "$OUTPUT" | jq -r '.approvals[0].signer')
    SIGNER_2=$(echo "$OUTPUT" | jq -r '.approvals[1].signer')

    if [ -z "$SIGNER_1" ] || [ "$SIGNER_1" == "null" ]; then
        log "ERROR: Missing first signer!"
        exit 1
    fi

    if [ -z "$SIGNER_2" ] || [ "$SIGNER_2" == "null" ]; then
        log "ERROR: Missing second signer!"
        exit 1
    fi

    log "... two signers found! [expected]"
    cp "$INPUT_FILE" "$OUTPUT_FILE"
}

# casper-client put-deploy
# ... sends deploy
# ... awaits for it to be included in a block
function test_put_deploy() {
    local CONTRACT_PATH=${1}
    local SIGNER_SECRET_KEY_PATH=${2}
    local PAYMENT
    local CHAIN_NAME
    local OUTPUT
    local DEPLOY_HASH

    log_step "Testing Client Subcommand: put-deploy"

    CHAIN_NAME=$(get_chain_name)
    PAYMENT='100000000000'

    OUTPUT=$($(get_path_to_client) put-deploy \
        --node-address "$(get_node_address_rpc)" \
        --chain-name "$CHAIN_NAME" \
        --payment-amount "$PAYMENT" \
        --session-path "$CONTRACT_PATH" \
        --ttl '5minutes' \
        --secret-key "$SIGNER_SECRET_KEY_PATH")

    # Check client responded
    test_with_jq "$OUTPUT"

    DEPLOY_HASH=$(echo "$OUTPUT" | jq -r '.result.deploy_hash')

    await_deploy_inclusion "$DEPLOY_HASH"
}

# casper-client make-deploy
# ... creates deploy
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_make_deploy() {
    local TRANSFER_TO=${1}
    local OUTPUT_PATH=${2}
    local SIGNER_PATH=${3}
    local CONTRACT_PATH
    local TRANSFER_TO_ACC_HASH
    local DEPLOY_FILE
    local AMOUNT
    local PAYMENT
    local SIGNER_SECRET_KEY_PATH
    local SIGNER_PUBLIC_HEX
    local CHAIN_NAME

    log_step "Testing Client Subcommand: make-deploy"

    CONTRACT_PATH=$(get_path_to_contract "transfers/transfer_to_account_u512.wasm")
    TRANSFER_TO_ACC_HASH=$(echo "$TRANSFER_TO" | awk -F'-' '{print $3}')
    DEPLOY_FILE="$OUTPUT_PATH/make_deploy.json"
    AMOUNT='2500000000'
    PAYMENT='100000000000'
    SIGNER_SECRET_KEY_PATH="$SIGNER_PATH/secret_key.pem"
    SIGNER_PUBLIC_HEX=$(cat "$SIGNER_PATH/public_key_hex")
    CHAIN_NAME=$(get_chain_name)

    mkdir -p "$OUTPUT_PATH"

    log "... creating transfer deploy file to -> $TRANSFER_TO"

    # Create deploy File
    $(get_path_to_client) make-deploy \
        --session-arg "$(get_cl_arg_u512 'amount' "$AMOUNT")" \
        --session-arg "$(get_cl_arg_account_hash 'target' "$TRANSFER_TO_ACC_HASH")" \
        --chain-name "$CHAIN_NAME" \
        --payment-amount "$PAYMENT" \
        --output "$DEPLOY_FILE" \
        --session-path "$CONTRACT_PATH" \
        --ttl '5minutes' \
        --secret-key "$SIGNER_SECRET_KEY_PATH"

    # Test Valid JSON
    if [ -f "$DEPLOY_FILE" ]; then
        cat "$DEPLOY_FILE" | jq '.'
    fi

    # Test Inputs Match Output
    # Signer
    if [ "$SIGNER_PUBLIC_HEX" = "$(jq -r '.approvals[].signer' $DEPLOY_FILE)" ]; then
        log "... signer match! [expected]"
    else
        log "ERROR: Mismatched signer!"
        exit 1
    fi

    # Amount
    if [ "$AMOUNT" = "$(jq -r '.session.ModuleBytes.args[0][1].parsed' $DEPLOY_FILE)" ]; then
        log "... amount match! [expected]"
    else
        log "ERROR: Mismatched amount!"
        exit 1
    fi

    # Account Hash
    if [ "$TRANSFER_TO_ACC_HASH" = "$(jq -r '.session.ModuleBytes.args[1][1].parsed' $DEPLOY_FILE)" ]; then
        log "... transfer to account hash match! [expected]"
    else
        log "ERROR: Mismatched transfer to account hash!"
        exit 1
    fi

    # Chain Name
    if [ "$CHAIN_NAME" = "$(jq -r '.header.chain_name' $DEPLOY_FILE)" ]; then
        log "... chain name match! [expected]"
    else
        log "ERROR: Mismatched chain name!"
        exit 1
    fi

    # Payment Amount
    if [ "$PAYMENT" = "$(jq -r '.payment.ModuleBytes.args[0][1].parsed' $DEPLOY_FILE)" ]; then
        log "... payment match! [expected]"
    else
        log "ERROR: Mismatched payment!"
        exit 1
    fi
}

# casper-client transfer
# ... sends deploy
# ... checks valid json with jq
# ... awaits for it to be included in a block
function test_transfer() {
    local TRANSFER_TO=${1}
    local SIGNER_PATH=${2}
    local TRANSFER_ID=${3}
    local TRANSFER_TO_ACC_HASH
    local AMOUNT
    local PAYMENT
    local SIGNER_SECRET_KEY_PATH
    local SIGNER_PUBLIC_HEX
    local CHAIN_NAME
    local DEPLOY_HASH
    local OUTPUT

    TRANSFER_TO_ACC_HASH=$(echo "$TRANSFER_TO" | awk -F'-' '{print $3}')
    AMOUNT='2500000000'
    PAYMENT='100000000000'
    SIGNER_SECRET_KEY_PATH="$SIGNER_PATH/secret_key.pem"
    SIGNER_PUBLIC_HEX=$(cat "$SIGNER_PATH/public_key_hex")
    CHAIN_NAME=$(get_chain_name)

    log_step "Testing Client Subcommand: transfer"

    OUTPUT=$($(get_path_to_client) transfer \
        --node-address "$(get_node_address_rpc)" \
        --amount "$AMOUNT" \
        --chain-name "$CHAIN_NAME" \
        --payment-amount "$PAYMENT" \
        --target-account "$TRANSFER_TO" \
        --secret-key "$SIGNER_SECRET_KEY_PATH" \
        --ttl '5minutes' \
        --transfer-id "$TRANSFER_ID"
    )

    # Check client responded
    test_with_jq "$OUTPUT"

    DEPLOY_HASH=$(echo "$OUTPUT" | jq -r '.result.deploy_hash')

    await_deploy_inclusion "$DEPLOY_HASH"
}

# casper-client get-account
# ... checks valid json with jq
# ... verifies some data matches expected outcome
# ... compare alias output
function test_get_account() {
    local PUBLIC_KEY=${1}
    local ACCOUNT_HASH=${2}
    local OUTPUT
    local ALIAS_OUTPUT

    log_step "Testing Client Subcommand: get-account"

    OUTPUT=$($(get_path_to_client) get-account \
        --node-address "$(get_node_address_rpc)" \
        --id '1' \
        --public-key "$PUBLIC_KEY")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Account Hash Check
    if [ "$ACCOUNT_HASH" = "$(echo $OUTPUT | jq -r '.result.account.account_hash')" ]; then
        log "... account hash match! [expected]"
    else
        log "ERROR: Mismatched balance!"
        exit 1
    fi

    log "Testing alias subcommand: get-account-info"

    ALIAS_OUTPUT=$($(get_path_to_client) get-account-info \
        --node-address "$(get_node_address_rpc)" \
        --id '1' \
        --public-key "$PUBLIC_KEY")

    # Check client responded
    test_with_jq "$ALIAS_OUTPUT"

    if [ "$OUTPUT" = "$ALIAS_OUTPUT" ]; then
        log "... alias output match! [expected]"
    else
        log "ERROR: Mismatched alias output!"
        exit 1
    fi
}

# casper-client query-balance
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_query_balance() {
    local BLOCK_HASH=${1}
    local TRANFER_TO_PUBLIC_KEY=${2}
    local BALANCE_TO=${3}
    local OUTPUT

    log_step "Testing Client Subcommand: query-balance"

    OUTPUT=$($(get_path_to_client) query-balance \
        --node-address "$(get_node_address_rpc)" \
        --block-identifier "$BLOCK_HASH" \
        --purse-identifier "$TRANFER_TO_PUBLIC_KEY")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Balance Check
    if [ "$BALANCE_TO" = "$(echo $OUTPUT | jq -r '.result.balance')" ]; then
        log "... balance match! [expected]"
    else
        log "ERROR: Mismatched balance!"
        exit 1
    fi
}

# casper-client query-global-state
# ... checks valid json with jq
# ... verifies some data matches expected outcome
# ... compare alias output
function test_query_global_state() {
    local BLOCK_HASH=${1}
    local DEPLOY_HASH=${2}
    local OUTPUT
    local ALIAS_OUTPUT

    log_step "Testing Client Subcommand: query-global-state"

    # Just tried testing different combinations
    # Probably a rabbit hole

    # --key deploy-"$DEPLOY_HASH"
    # --block-identifier "$BLOCK_HASH"
    OUTPUT=$($(get_path_to_client) query-global-state \
        --node-address "$(get_node_address_rpc)" \
        --block-identifier "$BLOCK_HASH" \
        --id '1' \
        --key deploy-"$DEPLOY_HASH")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Deploy Hash
    if [ "$DEPLOY_HASH" = "$(echo $OUTPUT | jq -r '.result.stored_value.DeployInfo.deploy_hash')" ]; then
        log "... deploy hash match! [expected]"
    else
        log "ERROR: Mismatched deploy hash!"
        exit 1
    fi

    log "Testing alias subcommand: query-state"

    ALIAS_OUTPUT=$($(get_path_to_client) query-state \
        --node-address "$(get_node_address_rpc)" \
        --block-identifier "$BLOCK_HASH" \
        --id '1' \
        --key deploy-"$DEPLOY_HASH")

    # Check client responded
    test_with_jq "$ALIAS_OUTPUT"

    if [ "$OUTPUT" = "$ALIAS_OUTPUT" ]; then
        log "... alias output match! [expected]"
    else
        log "ERROR: Mismatched alias output!"
        exit 1
    fi

    # --key deploy-"$DEPLOY_HASH"
    # --state-root-hash "$(get_state_root_hash)
    OUTPUT=$($(get_path_to_client) query-global-state \
        --node-address "$(get_node_address_rpc)" \
        --key deploy-"$DEPLOY_HASH" \
        --id '1' \
        --state-root-hash "$(get_state_root_hash)")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Deploy Hash
    if [ "$DEPLOY_HASH" = "$(echo $OUTPUT | jq -r '.result.stored_value.DeployInfo.deploy_hash')" ]; then
        log "... deploy hash match! [expected]"
    else
        log "ERROR: Mismatched deploy hash!"
        exit 1
    fi

    log "Testing alias subcommand: query-state"

    ALIAS_OUTPUT=$($(get_path_to_client) query-state \
        --node-address "$(get_node_address_rpc)" \
        --key deploy-"$DEPLOY_HASH" \
        --id '1' \
        --state-root-hash "$(get_state_root_hash)")

    # Check client responded
    test_with_jq "$ALIAS_OUTPUT"

    if [ "$OUTPUT" = "$ALIAS_OUTPUT" ]; then
        log "... alias output match! [expected]"
    else
        log "ERROR: Mismatched alias output!"
        exit 1
    fi
}

# casper-client get-era-info
# ... checks valid json with jq
# ... compare alias output
# TODO: check era_summary when selecting a switch block
#       for an era is an input
function test_get_era_info() {
    local BLOCK_HASH=${1}
    local OUTPUT
    local ALIAS_OUTPUT

    log_step "Testing Client Subcommand: get-era-info"

    OUTPUT=$($(get_path_to_client) get-era-info \
        --node-address "$(get_node_address_rpc)" \
        --id '1' \
        --block-identifier "$BLOCK_HASH")

    # Check client responded
    test_with_jq "$OUTPUT"

    log "Testing alias subcommand: get-era-info-by-switch-block"

    ALIAS_OUTPUT=$($(get_path_to_client) get-era-info-by-switch-block \
        --node-address "$(get_node_address_rpc)" \
         --id '1' \
        --block-identifier "$BLOCK_HASH")

    # Check client responded
    test_with_jq "$ALIAS_OUTPUT"

    if [ "$OUTPUT" = "$ALIAS_OUTPUT" ]; then
        log "... alias output match! [expected]"
    else
        log "ERROR: Mismatched alias output!"
        exit 1
    fi
}

# casper-client list-deploys
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_list_deploys() {
    local BLOCK_HASH=${1}
    local DEPLOY_HASH=${2}
    local OUTPUT

    log_step "Testing Client Subcommand: list-deploys"

    OUTPUT=$($(get_path_to_client) list-deploys \
        --node-address "$(get_node_address_rpc)" \
        --block-identifier "$BLOCK_HASH")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Deploy Hash
    if [ "$DEPLOY_HASH" = "$(echo $OUTPUT | jq -r '.transfer_hashes[]')" ]; then
        log "... transfer hash match! [expected]"
    else
        log "ERROR: Mismatched transfer hash!"
        exit 1
    fi
}

# casper-client get-block-transfers
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_get_block_transfers() {
    local BLOCK_HASH=${1}
    local DEPLOY_HASH=${2}
    local FROM_ACCOUNT=${3}
    local TO_ACCOUNT=${4}
    local OUTPUT

    log_step "Testing Client Subcommand: get-block-transfers"

    OUTPUT=$($(get_path_to_client) get-block-transfers \
        --node-address "$(get_node_address_rpc)" \
        --block-identifier "$BLOCK_HASH")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Validate
    # Block Hash
    if [ "$BLOCK_HASH" = "$(echo $OUTPUT | jq -r '.result.block_hash')" ]; then
        log "... block hash match! [expected]"
    else
        log "ERROR: Mismatched block hash!"
        exit 1
    fi

    # Deploy Hash
    if [ "$DEPLOY_HASH" = "$(echo $OUTPUT | jq -r '.result.transfers[].deploy_hash')" ]; then
        log "... deploy hash match! [expected]"
    else
        log "ERROR: Mismatched deploy hash!"
        exit 1
    fi

    # From Account
    if [ "$FROM_ACCOUNT" = "$(echo $OUTPUT | jq -r '.result.transfers[].from')" ]; then
        log "... from account match! [expected]"
    else
        log "ERROR: Mismatched from account!"
        exit 1
    fi

    # To Account
    if [ "$TO_ACCOUNT" = "$(echo $OUTPUT | jq -r '.result.transfers[].to')" ]; then
        log "... to account match! [expected]"
    else
        log "ERROR: Mismatched to account!"
        exit 1
    fi
}

# casper-client get-block
# ... checks valid json with jq
function test_get_block_by_hash() {
    local BLOCK_HASH=${1}
    local OUTPUT

    log_step "Testing Client Subcommand: get-block"

    OUTPUT=$($(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc)" \
        --block-identifier "$BLOCK_HASH")

    # Check client responded
    test_with_jq "$OUTPUT"
}

# casper-client get-deploy
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_get_deploy() {
    local DEPLOY_HASH=${1}
    local OUTPUT

    log_step "Testing Client Subcommand: get-deploy"

    OUTPUT=$($(get_path_to_client) get-deploy \
        --node-address "$(get_node_address_rpc)" \
        "$DEPLOY_HASH")

    # Check client responded
    test_with_jq "$OUTPUT"

    if [ "$DEPLOY_HASH" = "$(echo $OUTPUT | jq -r '.result.deploy.hash')" ]; then
        log "... deploy hash match! [expected]"
    else
        log "ERROR: Mismatched deploy hash!"
        exit 1
    fi
}

# casper-client send-deploy
# ... sends deploy
# ... checks valid json with jq
# ... awaits for it to be included in a block
function test_send_deploy() {
    local DEPLOY_PATH=${1}
    local OUTPUT

    log_step "Testing Client Subcommand: send-deploy"

    OUTPUT=$($(get_path_to_client) send-deploy \
        --input "$DEPLOY_PATH" \
        --node-address "$(get_node_address_rpc)")

    # Check client responded
    test_with_jq "$OUTPUT"

    # Verify Deploy makes it
    await_deploy_inclusion $(get_deploy_hash_from_file "$DEPLOY_PATH")
}

# casper-client make-transfer
# ... sends deploy
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_make_transfer() {
    local TRANSFER_TO=${1}
    local OUTPUT_PATH=${2}
    local SIGNER_PATH=${3}
    local TRANSFER_ID=${4}
    local TRANSFER_TO_ACC_HASH
    local DEPLOY_FILE
    local AMOUNT
    local PAYMENT
    local SIGNER_SECRET_KEY_PATH
    local SIGNER_PUBLIC_HEX
    local CHAIN_NAME

    log_step "Testing Client Subcommand: make-transfer"

    TRANSFER_TO_ACC_HASH=$(echo "$TRANSFER_TO" | awk -F'-' '{print $3}')
    DEPLOY_FILE="$OUTPUT_PATH/deploy_$TRANSFER_ID.json"
    AMOUNT='2500000000'
    PAYMENT='100000000000'
    SIGNER_SECRET_KEY_PATH="$SIGNER_PATH/secret_key.pem"
    SIGNER_PUBLIC_HEX=$(cat "$SIGNER_PATH/public_key_hex")
    CHAIN_NAME=$(get_chain_name)

    mkdir -p "$OUTPUT_PATH"

    log "... creating transfer deploy file to -> $TRANSFER_TO"

    # Create Transfer File
    $(get_path_to_client) make-transfer \
        --amount "$AMOUNT" \
        --chain-name "$CHAIN_NAME" \
        --payment-amount "$PAYMENT" \
        --target-account "$TRANSFER_TO" \
        --secret-key "$SIGNER_SECRET_KEY_PATH" \
        --transfer-id "$TRANSFER_ID" \
        --ttl '5minutes' \
        --output "$DEPLOY_FILE"

    # Test Valid JSON
    if [ -f "$DEPLOY_FILE" ]; then
        cat "$DEPLOY_FILE" | jq '.'
    fi

    # Test Inputs Match Output
    # Signer
    if [ "$SIGNER_PUBLIC_HEX" = "$(jq -r '.approvals[].signer' $DEPLOY_FILE)" ]; then
        log "... signer match! [expected]"
    else
        log "ERROR: Mismatched signer!"
        exit 1
    fi

    # Amount
    if [ "$AMOUNT" = "$(jq -r '.session.Transfer.args[0][1].parsed' $DEPLOY_FILE)" ]; then
        log "... amount match! [expected]"
    else
        log "ERROR: Mismatched amount!"
        exit 1
    fi

    # Account Hash
    if [ "$TRANSFER_TO_ACC_HASH" = "$(jq -r '.session.Transfer.args[1][1].parsed' $DEPLOY_FILE)" ]; then
        log "... transfer to account hash match! [expected]"
    else
        log "ERROR: Mismatched transfer to account hash!"
        exit 1
    fi

    # Tranfer ID
    if [ "$TRANSFER_ID" = "$(jq -r '.session.Transfer.args[2][1].parsed' $DEPLOY_FILE)" ]; then
        log "... transfer id match! [expected]"
    else
        log "ERROR: Mismatched transfer id!"
        exit 1
    fi

    # Chain Name
    if [ "$CHAIN_NAME" = "$(jq -r '.header.chain_name' $DEPLOY_FILE)" ]; then
        log "... chain name match! [expected]"
    else
        log "ERROR: Mismatched chain name!"
        exit 1
    fi

    # Payment Amount
    if [ "$PAYMENT" = "$(jq -r '.payment.ModuleBytes.args[0][1].parsed' $DEPLOY_FILE)" ]; then
        log "... payment match! [expected]"
    else
        log "ERROR: Mismatched payment!"
        exit 1
    fi
}

# casper-client account-address
# ... checks pem and hex output the same thing
function test_account_address() {
    local KEY_PATH=${1}
    local PEM_FILE
    local HEX_FILE
    local PEM_ACC_ADDR
    local HEX_ACC_ADDR

    log_step "Testing Client Subcommand: account-address"

    PEM_FILE="$KEY_PATH/public_key.pem"
    HEX_FILE="$KEY_PATH/public_key_hex"
    PEM_ACC_ADDR=$(get_account_hash "$PEM_FILE")
    HEX_ACC_ADDR=$(get_account_hash "$HEX_FILE")

    log "... Testing keys in $KEY_PATH"

    if [ "$PEM_ACC_ADDR" = "$HEX_ACC_ADDR" ]; then
        log "... PEM & HEX account address match! [expected]"
    else
        log "ERROR: PEM & HEX created a mismatched address!"
        exit 1
    fi
}

# casper-client get-chainspec
# ... checks output chainspec matches what NCTL is using
# ... checks output accounts matches what NCTL is using
# ... checks valid json with jq
function test_get_chainspec() {
    local OUTPUT_PATH=${1}

    log_step "Testing Client Subcommand: get-chainspec"

    mkdir -p "$OUTPUT_PATH"

    # Get config files from rpc
    get_chainspec_from_rpc > "$OUTPUT_PATH/rpc_chainspec.toml"
    get_accounts_from_rpc > "$OUTPUT_PATH/rpc_accounts.toml"
    # Test File From RPC matches whats running on a node
    compare_md5sum "$OUTPUT_PATH/rpc_chainspec.toml" "$(get_path_to_node 1)/config/1_0_0/chainspec.toml"
    compare_md5sum "$OUTPUT_PATH/rpc_accounts.toml" "$(get_path_to_node 1)/config/1_0_0/accounts.toml"
    # Test JSON OUTPUT is Valid, jq will balk if invalid json
    get_chainspec_json_from_rpc | jq '.'
}

# casper-client get-block
# ... test tip, block 1
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_get_block() {
    local BLOCK_HEIGHT
    local OUTPUT

    log_step "Testing Client Subcommand: get-block"

    OUTPUT=$($(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc)" \
        -vv)

    # Check client responded
    test_with_jq "$OUTPUT"

    # Specific block id = 1
    OUTPUT=$($(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc)" \
        -vv \
        -b 1)

    # Check client responded
    test_with_jq "$OUTPUT"

    # Check block id matches input block
    log "Test block height matches in RPC"
    BLOCK_HEIGHT=$($(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc)" \
        -b 1 \
        | jq -r '.result.block.header.height')

    if [ "$BLOCK_HEIGHT" = "1" ]; then
        log "... correct block height found! [expected]"
    else
        log "ERROR: Block Height Mismatch!"
        log "... $BLOCK_HEIGHT != 1"
        exit 1
    fi
}

# casper-client get-state-root-hash
# ... test tip, block 1
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_get_state_root_hash() {
    local BLOCK_HEIGHT
    local OUTPUT

    log_step "Testing Client Subcommand: get-state-root-hash"

    OUTPUT=$($(get_path_to_client) get-state-root-hash \
        --node-address "$(get_node_address_rpc)" \
        -vv)

    # Check client responded
    test_with_jq "$OUTPUT"

    # Specific block id = 1
    OUTPUT=$($(get_path_to_client) get-state-root-hash \
        --node-address "$(get_node_address_rpc)" \
        -vv \
        -b 1)

    # Check client responded
    test_with_jq "$OUTPUT"

    # Check block id matches input block
    log "Test block height matches in RPC"
    BLOCK_HEIGHT=$($(get_path_to_client) get-state-root-hash \
        --node-address "$(get_node_address_rpc)" \
        -vv \
        -b 1 \
        | jq -r '.params.block_identifier.Height | select( . != null )')

    if [ "$BLOCK_HEIGHT" = "1" ]; then
        log "... correct block height found! [expected]"
    else
        log "ERROR: Block Height Mismatch!"
        log "... $BLOCK_HEIGHT != 1"
        exit 1
    fi
}

# casper-client get-auction-info
# ... test tip, block 1
# ... checks valid json with jq
# ... verifies some data matches expected outcome
function test_get_auction_info() {
    local BLOCK_HEIGHT
    local OUTPUT

    log_step "Testing Client Subcommand: get-auction-info"

    OUTPUT=$($(get_path_to_client) get-auction-info \
        --node-address "$(get_node_address_rpc)" \
        -vv)

    # Check client responded
    test_with_jq "$OUTPUT"

    # Specific block id = 1
    OUTPUT=$($(get_path_to_client) get-auction-info \
        --node-address "$(get_node_address_rpc)" \
        -vv \
        -b 1)

    # Check client responded
    test_with_jq "$OUTPUT"

    # Check block id matches input block
    log "Test block height matches in RPC"
    BLOCK_HEIGHT=$($(get_path_to_client) get-auction-info \
        --node-address "$(get_node_address_rpc)" \
        -b 1 \
        | jq -r '.result.auction_state.block_height')

    if [ "$BLOCK_HEIGHT" = "1" ]; then
        log "... correct block height found! [expected]"
    else
        log "ERROR: Block Height Mismatch!"
        log "... $BLOCK_HEIGHT != 1"
        exit 1
    fi
}

# casper-client get-node-status
# ... checks valid json with jq
function test_get_node_status() {
    local OUTPUT
    log_step "Testing Client Subcommand: get-node-status"

    OUTPUT=$($(get_path_to_client) get-node-status \
        --node-address "$(get_node_address_rpc)" \
        -vv)

    # Check client responded
    test_with_jq "$OUTPUT"
}

# casper-client get-peers
# ... checks valid json with jq
function test_get_peers() {
    local OUTPUT
    log_step "Testing Client Subcommand: get-peers"

    # Piped it to JQ to make sure its proper json atleast
    OUTPUT=$($(get_path_to_client) get-peers \
        --node-address "$(get_node_address_rpc)" \
        -vv)

    # Check client responded
    test_with_jq "$OUTPUT"
}

# casper-client get-validator-changes
# ... checks valid json with jq
function test_get_validator_changes() {
    local OUTPUT
    log_step "Testing Client Subcommand: get-validator-changes"

    OUTPUT=$($(get_path_to_client) get-validator-changes \
        --node-address "$(get_node_address_rpc)" \
        -vv)

    # Check client responded
    test_with_jq "$OUTPUT"
}

# casper-client list-rpcs
# ... checks valid json with jq
function test_list_rpcs() {
    local OUTPUT
    log_step "Testing Client Subcommand: list-rpcs"

    # Piped it to JQ to make sure its proper json atleast
    OUTPUT=$($(get_path_to_client) list-rpcs \
        --node-address "$(get_node_address_rpc)" \
        -vv)

    # Check client responded
    test_with_jq "$OUTPUT"
}

# casper-client keygen
# ... generates key for each algorithm
# ... verify keygen creation
function test_keygen() {
    local OUTPUT_PATH=${1}
    local FILES
    local ALGOS

    log_step "Testing Client Subcommand: keygen"

    FILES=(public_key.pem public_key_hex secret_key.pem)
    ALGOS=($(get_keygen_algos))

    mkdir -p "$OUTPUT_PATH"

    for algo in "${ALGOS[@]}"; do
        local ALGO_PATH="$OUTPUT_PATH/$algo"
        log "... generating for: $algo"
        mkdir -p "$ALGO_PATH"
        $(get_path_to_client) keygen \
            -a "$algo" \
            "$ALGO_PATH"

        for file in "${FILES[@]}"; do
            if [ ! -f "$ALGO_PATH/$file" ]; then
                log "ERROR: no $file generated for $algo!"
                exit 1
            else
                log "ALGO: $algo - $file found!"
            fi
        done
    done
}

# casper-client generate-completion
# ... generates completion for each shell
# ... verifies completion file creation
function test_generate_completion() {
    local OUTPUT_PATH=${1}
    local SHELL_VALUES

    log_step "Testing Client Subcommand: generate-completion"

    SHELL_VALUES=($(get_generate_completion_shells))

    mkdir -p "$OUTPUT_PATH"

    for shell in "${SHELL_VALUES[@]}"; do
        log "... generating for: $shell"
        $(get_path_to_client) generate-completion \
            --output-dir "$OUTPUT_PATH" \
            --shell "$shell"

        if [ ! -f "$OUTPUT_PATH/casper-client" ]; then
            log "ERROR: no file generated!"
            exit 1
        else
            rm "$OUTPUT_PATH/casper-client"
        fi
    done
}

# casper-client <subcommand> --help
# ... trys each subcommand with --help flag
function test_subcommand_help() {
    local OUTPUT

    log_step "testing help output of all subcommands"

    OUTPUT=($(get_client_subcommand_list))

    # Check non-empty
    check_client_responded "${OUTPUT[*]}"

    for subcommand in "${OUTPUT[@]}"; do
        if [ "$subcommand" != "help" ]; then
            log "... testing: $subcommand --help"
            $(get_path_to_client) "$subcommand" --help #> /dev/null 2>&1
        fi
    done
}

#--------------------------------#
# HELPER FUNCTIONS USED IN TESTS #
#--------------------------------#

# returns uref from get-account by public key
function get_uref_for_public_key() {
    local PUBLIC_KEY=${1}
    local OUTPUT

    OUTPUT=$($(get_path_to_client) get-account \
        --node-address "$(get_node_address_rpc)" \
        --public-key "$PUBLIC_KEY")

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT" | jq -r '.result.account.main_purse'
}

# returns block hash containing deploy
function get_block_containing_deploy_hash() {
    local DEPLOY_HASH=${1}
    local OUTPUT

    OUTPUT=$($(get_path_to_client) get-deploy \
        --node-address "$(get_node_address_rpc)" \
        "$DEPLOY_HASH" | jq -r '.result.block_hash')

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns deploy hash from deploy file
function get_deploy_hash_from_file() {
    local DEPLOY_FILE_PATH=${1}
    local OUTPUT
    OUTPUT=$(jq -r '.hash' "$DEPLOY_FILE_PATH")

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns an account's hash
function get_account_hash() {
    local KEY=${1}
    local OUTPUT
    OUTPUT=$($(get_path_to_client) account-address \
        --public-key "$KEY")

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns only chainspec.toml from get-chainspec
function get_chainspec_from_rpc() {
    local OUTPUT
    OUTPUT=$($(get_path_to_client) get-chainspec \
        --node-address "$(get_node_address_rpc)" \
        -vv \
        | sed -n '/\[protocol\]/,$p' \
        | sed '/FAUCET/Q')

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns only accounts.toml from get-chainspec
function get_accounts_from_rpc() {
    local OUTPUT
    OUTPUT=$($(get_path_to_client) get-chainspec \
        --node-address "$(get_node_address_rpc)" \
        -vv \
        | sed -n '/FAUCET/,$p')

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns only the json output get-chainspec
function get_chainspec_json_from_rpc() {
    local OUTPUT
    OUTPUT=$($(get_path_to_client) get-chainspec \
        --node-address "$(get_node_address_rpc)" \
        -vv \
        | sed '/\[protocol\]/Q')

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns available algorithms from keygen
function get_keygen_algos() {
    local OUTPUT
    OUTPUT=$($(get_path_to_client) keygen --help \
        | grep 'possible values' \
        | cut -d "[" -f2 \
        | cut -d "]" -f1 \
        | awk -F'possible values: ' '{print $2}' \
        | tr -d "[:space:]" \
        |sed 's/,/ /g')

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns available shells from generate-completion
function get_generate_completion_shells() {
    local OUTPUT
    OUTPUT=$($(get_path_to_client) generate-completion --help \
        | awk -F'possible values:' '{print $2}' \
        | cut -d "]" -f1 \
        | tr -d "[:space:]" \
        | sed 's/,/ /g')

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# returns all client subcommands
function get_client_subcommand_list() {
    local OUTPUT
    OUTPUT=$($(get_path_to_client) --help \
        | awk -v RS= 'NR==3' \
        | sed -e '/^[ \t]\{3,\}/d' \
        | awk '{print $1}' \
        | sed -n '1!p' \
        | sort \
        | uniq)

    # Check non-empty
    check_client_responded "$OUTPUT"

    echo "$OUTPUT"
}

# counts client subcommands
function get_client_subcommand_count() {
    get_client_subcommand_list | wc -l
}

# compares 2 values
function compare_client_subcommand_count() {
    local COMP1=${1}
    local COMP2=${2}

    log_step "Comparing client subcommand count..."

    if [ "$COMP1" = "$COMP2" ]; then
        log "$COMP1 = $COMP2 [expected]"
    else
        log "ERROR: $COMP1 != $COMP2"
        log "... was a subcommand removed/added?"
        exit 1
    fi
}

# compares md5sum of 2 files
function compare_md5sum() {
    local COMP1=${1}
    local COMP2=${2}
    local SUM1
    local SUM2

    log "Comparing md5sum of $COMP1 & $COMP2"

    SUM1=$(md5sum "$COMP1" | awk '{print $1}')
    SUM2=$(md5sum "$COMP2" | awk '{print $1}')

    if [ "$SUM1" = "$SUM2" ]; then
        log "... Checksum matches!"
        log "... $SUM1 = $SUM2"
    else
        log "ERROR: Checksum mismatch!"
        log "... $SUM1 != $SUM2"
        exit 1
    fi
}

# waits for deploy hash to make it into block
function await_deploy_inclusion() {
    local DEPLOY_HASH=${1}
    local TIMEOUT=${2:-'300'}
    local GET_DEPLOY_OUTPUT

    while true; do
        log "... awaiting $DEPLOY_HASH inclusion: Timeout = $TIMEOUT"
        if [ "$TIMEOUT" = "0" ]; then
            log "ERROR: Timed out waiting for deploy: $DEPLOY_HASH"
            exit 1
        else
            GET_DEPLOY_OUTPUT=$($(get_path_to_client) get-deploy \
                --node-address "$(get_node_address_rpc)" \
                "$DEPLOY_HASH" | jq -r '.result.block_hash')
            if [ -z "$GET_DEPLOY_OUTPUT" ] || [ "$GET_DEPLOY_OUTPUT" == "null" ]; then
                sleep 1
                TIMEOUT=$((TIMEOUT-1))
            else
                log "... $DEPLOY_HASH included!"
                break
            fi
        fi
    done
}

# checks variable is not empty
function check_client_responded() {
    local CLIENT_OUTPUT=${1}

    # Make sure it is not empty
    if [ -z "$CLIENT_OUTPUT" ]; then
        log "Error: client didn't return anything!"
        exit 1
    fi
}

# checks json can be read by jq
function test_with_jq() {
    local JSON_OUTPUT=${1}

    check_client_responded "$JSON_OUTPUT"

    log "... testing client returned valid json"

    # Test Valid Json
    echo "$JSON_OUTPUT" | jq '.'
}

# cleans up test
function clean_tmp_dir() {
    log_step "Cleanup Temp Dir"
    if [ -d "/tmp/client" ]; then
        rm -rf /tmp/client
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset STEP
STEP=0

main
