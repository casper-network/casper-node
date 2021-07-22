/** @hidden */
@external("env", "casper_read_value")
export declare function read_value(key_ptr: usize, key_size: usize, value_size: usize): i32;
/** @hidden */
@external("env", "casper_dictionary_get")
export declare function dictionary_get(uref_ptr: usize, uref_ptr_size: usize, key_ptr: usize, key_size: usize, output_size: usize): i32;
/** @hidden */
@external("env", "casper_write")
export declare function write(key_ptr: usize, key_size: usize, value_ptr: usize, value_size: usize): void;
/** @hidden */
@external("env", "casper_dictionary_put")
export declare function dictionary_put(uref_ptr: usize, uref_ptr_size: usize, key_ptr: usize, key_size: usize, value_ptr: usize, value_size: usize): i32;
/** @hidden */
@external("env", "casper_add")
export declare function add(key_ptr: usize, key_size: usize, value_ptr: usize, value_size: usize): void;
/** @hidden */
@external("env", "casper_new_uref")
export declare function new_uref(uref_ptr: usize, value_ptr: usize, value_size: usize): void;
@external("env", "casper_load_named_keys")
export declare function load_named_keys(total_keys: usize, result_size: usize): i32;
/** @hidden */
@external("env", "casper_get_named_arg")
export declare function get_named_arg(name_ptr: usize, name_size: usize, dest_ptr: usize, dest_size: usize): i32;
/** @hidden */
@external("env", "casper_get_named_arg_size")
export declare function get_named_arg_size(name_ptr: usize, name_size: usize, dest_size: usize): i32;
/** @hidden */
@external("env", "casper_ret")
export declare function ret(value_ptr: usize, value_size: usize): void;
/** @hidden */
@external("env", "casper_call_contract")
export declare function call_contract(contract_hash_ptr: usize, contract_hash_size: usize, entry_point_name_ptr: usize, entry_point_name_size: usize, runtime_args_ptr: usize, runtime_args_size: usize, result_size: usize): i32;
/** @hidden */
@external("env", "casper_call_versioned_contract")
export declare function call_versioned_contract(
    contract_package_hash_ptr: usize,
    contract_package_hash_size: usize,
    contract_version_ptr: usize,
    contract_version_size: usize,
    entry_point_name_ptr: usize,
    entry_point_name_size: usize,
    runtime_args_ptr: usize,
    runtime_args_size: usize,
    result_size: usize,
): i32;
/** @hidden */
@external("env", "casper_get_key")
export declare function get_key(
    name_ptr: usize,
    name_size: usize,
    output_ptr: usize,
    output_size: usize,
    bytes_written_ptr: usize,
): i32;
/** @hidden */
@external("env", "casper_has_key")
export declare function has_key(name_ptr: usize, name_size: usize): i32;
/** @hidden */
@external("env", "casper_put_key")
export declare function put_key(name_ptr: usize, name_size: usize, key_ptr: usize, key_size: usize): void;
/** @hidden */
@external("env", "casper_remove_key")
export declare function remove_key(name_ptr: usize, name_size: u32): void;
/** @hidden */
@external("env", "casper_revert")
export declare function revert(err_code: i32): void;
/** @hidden */
@external("env", "casper_is_valid_uref")
export declare function is_valid_uref(target_ptr: usize, target_size: u32): i32;
/** @hidden */
@external("env", "casper_add_associated_key")
export declare function add_associated_key(account_hash_ptr: usize, account_hash_size: usize, weight: i32): i32;
/** @hidden */
@external("env", "casper_remove_associated_key")
export declare function remove_associated_key(account_hash_ptr: usize, account_hash_size: usize): i32;
/** @hidden */
@external("env", "casper_update_associated_key")
export declare function update_associated_key(account_hash_ptr: usize, account_hash_size: usize, weight: i32): i32;
/** @hidden */
@external("env", "casper_set_action_threshold")
export declare function set_action_threshold(permission_level: u32, threshold: i32): i32;
/** @hidden */
@external("env", "casper_get_blocktime")
export declare function get_blocktime(dest_ptr: usize): void;
/** @hidden */
@external("env", "casper_get_caller")
export declare function get_caller(output_size: usize): i32;
/** @hidden */
@external("env", "casper_create_purse")
export declare function create_purse(purse_ptr: usize, purse_size: u32): i32;
/** @hidden */
@external("env", "casper_transfer_to_account")
export declare function transfer_to_account(
    target_ptr: usize,
    target_size: u32,
    amount_ptr: usize,
    amount_size: u32,
    id_ptr: usize,
    id_size: u32,
    result_ptr: usize,
): i32;
/** @hidden */
@external("env", "casper_transfer_from_purse_to_account")
export declare function transfer_from_purse_to_account(
    source_ptr: usize,
    source_size: u32,
    target_ptr: usize,
    target_size: u32,
    amount_ptr: usize,
    amount_size: u32,
    id_ptr: usize,
    id_size: u32,
    result_ptr: usize,
):  i32;
/** @hidden */
@external("env", "casper_transfer_from_purse_to_purse")
export declare function transfer_from_purse_to_purse(
    source_ptr: usize,
    source_size: u32,
    target_ptr: usize,
    target_size: u32,
    amount_ptr: usize,
    amount_size: u32,
    id_ptr: usize,
    id_size: u32,
): i32;
/** @hidden */
@external("env", "casper_get_balance")
export declare function get_balance(purse_ptr: usize, purse_size: usize, result_size: usize): i32;
/** @hidden */
@external("env", "casper_get_phase")
export declare function get_phase(dest_ptr: usize): void;
/** @hidden */
@external("env", "casper_upgrade_contract_at_uref")
export declare function upgrade_contract_at_uref(
    name_ptr: usize,
    name_size: u32,
    key_ptr: usize,
    key_size: u32
): i32;
/** @hidden */
@external("env", "casper_get_system_contract")
export declare function get_system_contract(system_contract_index: u32, dest_ptr: usize, dest_size: u32): i32;
/** @hidden */
@external("env", "casper_get_main_purse")
export declare function get_main_purse(dest_ptr: usize): void;
/** @hidden */
@external("env", "casper_read_host_buffer")
export declare function read_host_buffer(dest_ptr: usize, dest_size: u32, bytes_written: usize): i32;
/** @hidden */
@external("env", "casper_remove_contract_user_group")
export declare function remove_contract_user_group(
    contract_package_hash_ptr: usize,
    contract_package_hash_size: usize,
    label_ptr: usize,
    label_size: usize): i32;
/** @hidden */
@external("env", "casper_provision_contract_user_group_uref")
export declare function provision_contract_user_group_uref(
    contract_package_hash_ptr: usize,
    contract_package_hash_size: usize,
    label_ptr: usize,
    label_size: usize,
    value_size_ptr: usize,
): i32;
/** @hidden */
@external("env", "casper_remove_contract_user_group_urefs")
export declare function remove_contract_user_group_urefs(
    contract_package_hash_ptr: usize,
    contract_package_hash_size: usize,
    label_ptr: usize,
    label_size: usize,
    urefs_ptr: usize,
    urefs_size: usize,
): i32;
/** @hidden */
@external("env", "casper_create_contract_package_at_hash")
export declare function create_contract_package_at_hash(hash_addr_ptr: usize, access_addr_ptr: usize, is_locked: boolean): void;
/** @hidden */
@external("env", "casper_add_contract_version")
export declare function add_contract_version(
    contract_package_hash_ptr: usize,
    contract_package_hash_size: usize,
    version_ptr: usize,
    entry_points_ptr: usize,
    entry_points_size: usize,
    named_keys_ptr: usize,
    named_keys_size: usize,
    output_ptr: usize,
    output_size: usize,
    bytes_written_ptr: usize,
): i32;
/** @hidden */
@external("env", "casper_create_contract_user_group")
export declare function create_contract_user_group(
    contract_package_hash_ptr: usize,
    contract_package_hash_size: usize,
    label_ptr: usize,
    label_size: usize,
    num_new_urefs: u8,
    existing_urefs_ptr: usize,
    existing_urefs_size: usize,
    output_size_ptr: usize,
): i32;
/** @hidden */
@external("env", "casper_disable_contract_version")
export declare function disable_contract_version(
    contract_package_hash_ptr: usize,
    contract_package_hash_size: usize,
    contract_hash_ptr: usize,
    contract_hash_size: usize,
): i32;
@external("env", "casper_new_dictionary")
export declare function casper_new_dictionary(output_size_ptr: usize): i32;