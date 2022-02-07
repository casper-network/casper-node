//! Contains low-level bindings for host-side ("external") functions.
//!
//! Generally should not be used directly.  See the [`contract_api`](crate::contract_api) for
//! high-level bindings suitable for writing smart contracts.
extern "C" {
    /// The bytes in the span of wasm memory from `key_ptr` to `key_ptr + key_size` must correspond
    /// to a valid global state key, otherwise the function will fail. If the key is de-serialized
    /// successfully, then the result of the read is serialized and buffered in the runtime. This
    /// result can be obtained via the [`casper_read_host_buffer`] function. Returns standard error
    /// code.
    ///
    /// # Arguments
    ///
    /// * `key_ptr` - pointer (offset in wasm linear memory) to serialized form of the key to read
    /// * `key_size` - size of the serialized key (in bytes)
    /// * `output_size` - pointer to a value where host will write size of bytes read from given key
    pub fn casper_read_value(key_ptr: *const u8, key_size: usize, output_size: *mut usize) -> i32;
    /// This function writes the provided value (read via de-serializing the bytes
    /// in wasm memory from offset `value_ptr` to `value_ptr + value_size`) under
    /// the provided key (read via de-serializing the bytes in wasm memory from
    /// offset `key_ptr` to `key_ptr + key_size`) in the global state. This
    /// function will cause a `Trap` if the key or value fail to de-serialize or
    /// if writing to that key is not permitted.
    ///
    /// # Arguments
    ///
    /// * `key_ptr` - pointer to bytes representing the key to write to
    /// * `key_size` - size of the key (in bytes)
    /// * `value_ptr` - pointer to bytes representing the value to write at the key
    /// * `value_size` - size of the value (in bytes)
    pub fn casper_write(
        key_ptr: *const u8,
        key_size: usize,
        value_ptr: *const u8,
        value_size: usize,
    );
    /// This function adds the provided value (read via de-serializing the bytes
    /// in wasm memory from offset `value_ptr` to `value_ptr + value_size`) to the
    /// current value under the provided key (read via de-serializing the bytes in
    /// wasm memory from offset `key_ptr` to `key_ptr + key_size`) in the global
    /// state. This function will cause a `Trap` if the key or value fail to
    /// de-serialize or if adding to that key is not permitted, or no value
    /// presently exists at that key.
    ///
    /// # Arguments
    ///
    /// * `key_ptr` - pointer to bytes representing the key to write to
    /// * `key_size` - size of the key (in bytes)
    /// * `value_ptr` - pointer to bytes representing the value to write at the key
    /// * `value_size` - size of the value (in bytes)
    pub fn casper_add(key_ptr: *const u8, key_size: usize, value_ptr: *const u8, value_size: usize);
    /// This function causes the runtime to generate a new `URef`, with
    /// the provided value stored under it in the global state. The new
    /// `URef` is written (in serialized form) to the wasm linear
    /// memory starting from the `key_ptr` offset. Note that data corruption is possible if not
    /// enough memory is allocated for the `URef` at `key_ptr`. This
    /// function will cause a `Trap` if the bytes in wasm memory from offset `value_ptr` to
    /// `value_ptr + value_size` cannot be de-serialized into a `Value`.
    ///
    /// # Arguments
    ///
    /// * `key_ptr` - pointer to the offset in wasm memory where the new `URef` will be written
    /// * `value_ptr` - pointer to bytes representing the value to write under the new `URef`
    /// * `value_size` - size of the value (in bytes)
    pub fn casper_new_uref(uref_ptr: *mut u8, value_ptr: *const u8, value_size: usize);
    /// This function loads a set of authorized keys used to sign this deploy from the host.
    /// The data will be available through the host buffer and can be copied to Wasm memory through
    /// [`casper_read_host_buffer`].
    ///
    /// # Arguments
    ///
    /// * `total_keys`: number of authorization keys used to sign this deploy
    /// * `result_size`: size of the data loaded in the host
    pub fn casper_load_authorization_keys(total_keys: *mut usize, result_size: *mut usize) -> i32;
    ///
    pub fn casper_load_named_keys(total_keys: *mut usize, result_size: *mut usize) -> i32;
    /// This function causes a `Trap`, terminating the currently running module,
    /// but first copies the bytes from `value_ptr` to `value_ptr + value_size` to
    /// a buffer which is returned to the calling module (if this module was
    /// invoked by [`casper_call_contract`] or [`casper_call_versioned_contract`]). Additionally,
    /// the known `URef`s of the calling context are augmented with the
    /// `URef`s de-serialized from wasm memory offset
    /// `extra_urefs_ptr` to `extra_urefs_ptr + extra_urefs_size`. This function will cause a
    /// `Trap` if the bytes at `extra_urefs_ptr` cannot be de-serialized as type `Vec<URef>`, or
    /// if any of the extra `URef`s are invalid in the current
    /// context.
    ///
    /// # Arguments
    ///
    /// * `value_ptr`: pointer to bytes representing the value to return to the caller
    /// * `value_size`: size of the value (in bytes)
    pub fn casper_ret(value_ptr: *const u8, value_size: usize) -> !;
    ///
    pub fn casper_get_key(
        name_ptr: *const u8,
        name_size: usize,
        output_ptr: *mut u8,
        output_size: usize,
        bytes_written_ptr: *mut usize,
    ) -> i32;
    ///
    pub fn casper_has_key(name_ptr: *const u8, name_size: usize) -> i32;
    ///
    pub fn casper_put_key(
        name_ptr: *const u8,
        name_size: usize,
        key_ptr: *const u8,
        key_size: usize,
    );
    ///
    pub fn casper_remove_key(name_ptr: *const u8, name_size: usize);
    /// This function causes a `Trap` which terminates the currently running
    /// module. Additionally, it signals that the current entire phase of
    /// execution of the deploy should be terminated as well, and that the effects
    /// of the execution up to this point should be reverted. The error code
    /// provided to this function will be included in the error message of the
    /// deploy in the block in which it is included.
    ///
    /// # Arguments
    ///
    /// * `status` - error code of the revert
    pub fn casper_revert(status: u32) -> !;
    /// This function checks if all the keys contained in the given `Value` are
    /// valid in the current context (i.e. the `Value` does not contain any forged
    /// `URef`s). This function causes a `Trap` if the bytes in wasm
    /// memory from offset `value_ptr` to `value_ptr + value_size` cannot be de-serialized as
    /// type `Value`.
    pub fn casper_is_valid_uref(uref_ptr: *const u8, uref_size: usize) -> i32;
    /// This function attempts to add the given public key as an associated key to
    /// the current account. Presently only 32-byte keys are supported; it is up
    /// to the caller to ensure that the 32-bytes starting from offset
    /// `public_key` represent the key they wish to add. Weights are internally
    /// represented by a `u8`, this function will cause a `Trap` if the weight is
    /// not between 0 and 255 inclusively. The result returned is a status code
    /// for adding the key where 0 represents success, 1 means no more keys can be
    /// added to this account (only 10 keys can be added), 2 means the key is
    /// already associated (if you wish to change the weight of an associated key
    /// then used [`casper_update_associated_key`]), and 3 means permission denied (this
    /// could be because the function was called outside of session code or
    /// because the key management threshold was not met by the keys authorizing
    /// the deploy).
    ///
    /// Returns status code for adding the key, where 0 represents success and non-zero represents
    /// failure.
    ///
    /// # Arguments
    ///
    /// * `public_key` - pointer to the bytes in wasm memory representing the public key to add,
    ///   presently only 32-byte public keys are supported.
    /// * `weight` - the weight to assign to this public key
    pub fn casper_add_associated_key(
        account_hash_ptr: *const u8,
        account_hash_size: usize,
        weight: i32,
    ) -> i32;
    /// This function attempts to remove the given public key from the associated
    /// keys of the current account. Presently only 32-byte keys are supported; it
    /// is up to the caller to ensure that the 32-bytes starting from offset
    /// `public_key` represent the key they wish to remove. The result returned is
    /// a status code for adding the key where 0 represents success, 1 means the
    /// key was not associated to begin with, 2 means means permission denied
    /// (this could be because the function was called outside of session code or
    /// because the key management threshold was not met by the keys authorizing
    /// the deploy), and 3 means this key cannot be removed because otherwise it
    /// would be impossible to meet either the deploy or key management
    /// thresholds.
    ///
    /// Returns status code for adding the key, where 0 represents success and non-zero represents
    /// failure.
    ///
    /// # Arguments
    ///
    /// * `public_key` - pointer to the bytes in wasm memory representing the public key to update,
    ///   presently only 32-byte public keys are supported.
    /// * `weight` - the weight to assign to this public key
    pub fn casper_remove_associated_key(
        account_hash_ptr: *const u8,
        account_hash_size: usize,
    ) -> i32;
    /// This function attempts to update the given public key as an associated key
    /// to the current account. Presently only 32-byte keys are supported; it is
    /// up to the caller to ensure that the 32-bytes starting from offset
    /// `public_key` represent the key they wish to add. Weights are internally
    /// represented by a `u8`, this function will cause a `Trap` if the weight is
    /// not between 0 and 255 inclusively. The result returned is a status code
    /// for adding the key where 0 represents success, 1 means the key was not
    /// associated to the account (to add a new key use `add_associated_key`), 2
    /// means means permission denied (this could be because the function was
    /// called outside of session code or because the key management threshold was
    /// not met by the keys authorizing the deploy), and 3 means this key cannot
    /// be changed to the specified weight because then it would be impossible to
    /// meet either the deploy or key management thresholds (you may wish to try
    /// again with a higher weight or after lowering the action thresholds).
    ///
    /// # Arguments
    ///
    /// * `public_key` - pointer to the bytes in wasm memory representing the
    ///  public key to update, presently only 32-byte public keys are supported
    /// * `weight` - the weight to assign to this public key
    pub fn casper_update_associated_key(
        account_hash_ptr: *const u8,
        account_hash_size: usize,
        weight: i32,
    ) -> i32;
    /// This function changes the threshold to perform the specified action. The
    /// action index is interpreted as follows: 0 means deployment and 1 means key
    /// management. Thresholds are represented internally as a `u8`, this function
    /// will cause a `Trap` if the new threshold is not between 0 and 255
    /// inclusively. The return value is a status code where 0 means success, 1
    /// means the key management threshold cannot be set lower than the deploy
    /// threshold, 2 means the deployment threshold cannot be set higher than the
    /// key management threshold, 3 means permission denied (this could be because
    /// the function was called outside of session code or because the key
    /// management threshold was not met by the keys authorizing the deploy), and
    /// 4 means the threshold would be set higher than the total weight of
    /// associated keys (and therefore would be impossible to meet).
    ///
    /// # Arguments
    ///
    /// * `action` - index representing the action threshold to set
    /// * `threshold` - new value of the threshold for performing this action
    pub fn casper_set_action_threshold(permission_level: u32, threshold: u32) -> i32;
    /// This function returns the public key of the account for this deploy. The
    /// result is always 36-bytes in length (4 bytes prefix on a 32-byte public
    /// key); it is up to the caller to ensure the right amount of memory is
    /// allocated at `dest_ptr`, data corruption in the wasm memory could occur
    /// otherwise.
    ///
    /// # Arguments
    ///
    /// * `dest_ptr` - pointer to position in wasm memory where to write the result
    pub fn casper_get_caller(output_size: *mut usize) -> i32;
    /// This function gets the timestamp which will be in the block this deploy is
    /// included in. The return value is always a 64-bit unsigned integer,
    /// representing the number of milliseconds since the Unix epoch. It is up to
    /// the caller to ensure there are 8 bytes allocated at `dest_ptr`, otherwise
    /// data corruption in the wasm memory may occur.
    ///
    /// # Arguments
    ///
    /// * `dest_ptr` - pointer in wasm memory where to write the result
    pub fn casper_get_blocktime(dest_ptr: *const u8);
    /// This function uses the mint contract to create a new, empty purse. If the
    /// call is successful then the `URef` (in serialized form) is written
    /// to the indicated place in wasm memory. It is up to the caller to ensure at
    /// least `purse_size` bytes are allocated at `purse_ptr`, otherwise
    /// data corruption may occur. This function causes a `Trap` if
    /// `purse_size` is not equal to 38.
    ///
    /// # Arguments
    ///
    /// * `purse_ptr` - pointer to position in wasm memory where to write the created `URef`
    /// * `purse_size` - allocated size for the `URef`
    pub fn casper_create_purse(purse_ptr: *const u8, purse_size: usize) -> i32;
    /// This function uses the mint contract’s transfer function to transfer
    /// tokens from the current account’s main purse to the main purse of the
    /// target account. If the target account does not exist then it is
    /// automatically created, and the tokens are transferred to the main purse of
    /// the new account. The target is a serialized `PublicKey` (i.e. 36 bytes
    /// where the first 4 bytes are the number `32` in little endian encoding, and
    /// the remaining 32-bytes are the public key). The amount must be a
    /// serialized 512-bit unsigned integer. This function causes a `Trap` if the
    /// target cannot be de-serialized as a `PublicKey` or the amount cannot be
    /// de-serialized into a `U512`. The return value indicated what occurred,
    /// where 0 means a successful transfer to an existing account, 1 means a
    /// successful transfer to a new account, and 2 means the transfer failed
    /// (this could be because the current account’s main purse had insufficient
    /// tokens or because the function was called outside of session code and so
    /// does not have access to the account’s main purse).
    ///
    /// # Arguments
    ///
    /// * `target_ptr` - pointer in wasm memory to bytes representing the target account to transfer
    ///   to
    /// * `target_size` - size of the target (in bytes)
    /// * `amount_ptr` - pointer in wasm memory to bytes representing the amount to transfer to the
    ///   target account
    /// * `amount_size` - size of the amount (in bytes)
    /// * `id_ptr` - pointer in wasm memory to bytes representing the user-defined transaction id
    /// * `id_size` - size of the id (in bytes)
    /// * `result_ptr` - pointer in wasm memory to a value where `TransferredTo` value would be set
    ///   on successful transfer.
    pub fn casper_transfer_to_account(
        target_ptr: *const u8,
        target_size: usize,
        amount_ptr: *const u8,
        amount_size: usize,
        id_ptr: *const u8,
        id_size: usize,
        result_ptr: *const i32,
    ) -> i32;
    /// This function uses the mint contract’s transfer function to transfer
    /// tokens from the specified purse to the main purse of the target account.
    /// If the target account does not exist then it is automatically created, and
    /// the tokens are transferred to the main purse of the new account. The
    /// source is a serialized `URef`.
    /// The target is a serialized `PublicKey` (i.e. 36 bytes where the
    /// first 4 bytes are the number `32` in little endian encoding, and the
    /// remaining 32-bytes are the public key). The amount must be a serialized
    /// 512-bit unsigned integer. This function causes a `Trap` if the source
    /// cannot be de-serialized as a `URef`, or the target cannot be
    /// de-serialized as a `PublicKey` or the amount cannot be de-serialized into
    /// a `U512`. The return value indicated what occurred, where 0 means a
    /// successful transfer to an existing account, 1 means a successful transfer
    /// to a new account, and 2 means the transfer failed (this could be because
    /// the source purse had insufficient tokens or because there was not valid
    /// access to the source purse).
    ///
    /// # Arguments
    ///
    /// * `source_ptr` - pointer in wasm memory to bytes representing the source `URef` to transfer
    ///   from
    /// * `source_size` - size of the source `URef` (in bytes)
    /// * `target_ptr` - pointer in wasm memory to bytes representing the target account to transfer
    ///   to
    /// * `target_size` - size of the target (in bytes)
    /// * `amount_ptr` - pointer in wasm memory to bytes representing the amount to transfer to the
    ///   target account
    /// * `amount_size` - size of the amount (in bytes)
    /// * `id_ptr` - pointer in wasm memory to bytes representing the user-defined transaction id
    /// * `id_size` - size of the id (in bytes)
    /// * `result_ptr` - pointer in wasm memory to a value where `TransferredTo` value would be set
    ///   on successful transfer.
    pub fn casper_transfer_from_purse_to_account(
        source_ptr: *const u8,
        source_size: usize,
        target_ptr: *const u8,
        target_size: usize,
        amount_ptr: *const u8,
        amount_size: usize,
        id_ptr: *const u8,
        id_size: usize,
        result_ptr: *const i32,
    ) -> i32;
    /// This function uses the mint contract’s transfer function to transfer
    /// tokens from the specified source purse to the specified target purse. If
    /// the target account does not exist then it is automatically created, and
    /// the tokens are transferred to the main purse of the new account. The
    /// source is a serialized `URef`.
    /// The target is also a serialized `URef`. The amount must be a
    /// serialized 512-bit unsigned integer. This function causes a `Trap` if the
    /// source or target cannot be de-serialized as a `URef` or the amount
    /// cannot be de-serialized into a `U512`. The return value indicated what
    /// occurred, where 0 means a successful transfer, 1 means the transfer
    /// failed (this could be because the source purse had insufficient tokens or
    /// because there was not valid access to the source purse or target purse).
    ///
    /// # Arguments
    ///
    /// * `source_ptr` - pointer in wasm memory to bytes representing the source `URef` to transfer
    ///   from
    /// * `source_size` - size of the source `URef` (in bytes)
    /// * `target_ptr` - pointer in wasm memory to bytes representing the target `URef` to transfer
    ///   to
    /// * `target_size` - size of the target (in bytes)
    /// * `amount_ptr` - pointer in wasm memory to bytes representing the amount to transfer to the
    ///   target account
    /// * `amount_size` - size of the amount (in bytes)
    /// * `id_ptr` - pointer in wasm memory to bytes representing the user-defined transaction id
    /// * `id_size` - size of the id (in bytes)
    pub fn casper_transfer_from_purse_to_purse(
        source_ptr: *const u8,
        source_size: usize,
        target_ptr: *const u8,
        target_size: usize,
        amount_ptr: *const u8,
        amount_size: usize,
        id_ptr: *const u8,
        id_size: usize,
    ) -> i32;
    /// Records a transfer.  Can only be called from within the mint contract.
    /// Needed to support system contract-based execution.
    ///
    /// # Arguments
    ///
    /// * `maybe_to_ptr` - pointer in wasm memory to bytes representing the recipient
    ///   `Option<AccountHash>`
    /// * `maybe_to_size` - size of the source `Option<AccountHash>` (in bytes)
    /// * `source_ptr` - pointer in wasm memory to bytes representing the source `URef` to transfer
    ///   from
    /// * `source_size` - size of the source `URef` (in bytes)
    /// * `target_ptr` - pointer in wasm memory to bytes representing the target `URef` to transfer
    ///   to
    /// * `target_size` - size of the target (in bytes)
    /// * `amount_ptr` - pointer in wasm memory to bytes representing the amount to transfer to the
    ///   target account
    /// * `amount_size` - size of the amount (in bytes)
    /// * `id_ptr` - pointer in wasm memory to bytes representing the user-defined transaction id
    /// * `id_size` - size of the id (in bytes)
    pub fn casper_record_transfer(
        maybe_to_ptr: *const u8,
        maybe_to_size: usize,
        source_ptr: *const u8,
        source_size: usize,
        target_ptr: *const u8,
        target_size: usize,
        amount_ptr: *const u8,
        amount_size: usize,
        id_ptr: *const u8,
        id_size: usize,
    ) -> i32;
    /// Records era info.  Can only be called from within the auction contract.
    /// Needed to support system contract-based execution.
    ///
    /// # Arguments
    ///
    /// * `era_id_ptr` - pointer in wasm memory to bytes representing the `EraId`
    /// * `era_id_size` - size of the `EraId` (in bytes)
    /// * `era_info_ptr` - pointer in wasm memory to bytes representing the `EraInfo`
    /// * `era_info_size` - size of the `EraInfo` (in bytes)
    pub fn casper_record_era_info(
        era_id_ptr: *const u8,
        era_id_size: usize,
        era_info_ptr: *const u8,
        era_info_size: usize,
    ) -> i32;
    /// This function uses the mint contract's balance function to get the balance
    /// of the specified purse. It causes a `Trap` if the bytes in wasm memory
    /// from `purse_ptr` to `purse_ptr + purse_size` cannot be
    /// de-serialized as a `URef`. The return value is the size of the
    /// result in bytes. The result is copied to the host buffer and thus can be obtained
    /// by any function which copies the buffer into wasm memory (e.g.
    /// `get_read`). The result bytes are serialized from type `Option<U512>` and
    /// should be interpreted as such.
    ///
    /// # Arguments
    ///
    /// * `purse_ptr` - pointer in wasm memory to the bytes representing the `URef` of the purse to
    ///   get the balance of
    /// * `purse_size` - size of the `URef` (in bytes)
    pub fn casper_get_balance(
        purse_ptr: *const u8,
        purse_size: usize,
        result_size: *mut usize,
    ) -> i32;
    /// This function writes bytes representing the current phase of the deploy
    /// execution to the specified pointer. The size of the result is always one
    /// byte, it is up to the caller to ensure one byte of memory is allocated at
    /// `dest_ptr`, otherwise data corruption in the wasm memory could occur. The
    /// one byte is interpreted as follows: 0 means a system phase (should never
    /// be encountered by user deploys), 1 means the payment phase, 2 means the
    /// session phase and 3 means the finalization phase (should never be
    /// encountered by user code).
    ///
    /// # Arguments
    ///
    /// * `dest_ptr` - pointer to position in wasm memory to write the result
    pub fn casper_get_phase(dest_ptr: *mut u8);
    ///
    pub fn casper_get_system_contract(
        system_contract_index: u32,
        dest_ptr: *mut u8,
        dest_size: usize,
    ) -> i32;
    ///
    pub fn casper_get_main_purse(dest_ptr: *mut u8);
    /// This function copies the contents of the current runtime buffer into the
    /// wasm memory, beginning at the provided offset. It is intended that this
    /// function be called after a call to a function that uses host buffer. It is up to the caller
    /// to ensure that the proper amount of memory is allocated for this write,
    /// otherwise data corruption in the wasm memory may occur due to this call
    /// overwriting some bytes unintentionally. The size of the data which will be
    /// written is stored on the host. The bytes which are written are those corresponding to the
    /// value returned by the called contract; it is up to the developer to know how to attempt
    /// to interpret those bytes.
    ///
    /// # Arguments
    ///
    /// * `dest_ptr` - pointer (offset in wasm memory) to the location where the host buffer should
    ///   be written
    /// * `dest_size` - size of output buffer
    /// * `bytes_written` - a pointer to a value where amount of bytes written will be set
    pub fn casper_read_host_buffer(
        dest_ptr: *mut u8,
        dest_size: usize,
        bytes_written: *mut usize,
    ) -> i32;
    /// Creates new contract package at hash. Returns both newly generated
    /// [`casper_types::ContractPackageHash`] and a [`casper_types::URef`] for further
    /// modifying access.
    pub fn casper_create_contract_package_at_hash(
        hash_addr_ptr: *mut u8,
        access_addr_ptr: *mut u8,
        is_locked: bool,
    );
    /// Creates new named contract user group under a contract package.
    ///
    /// # Arguments
    ///
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `label_ptr` - serialized group label
    /// * `label_size` - size of serialized group label
    /// * `num_new_urefs` - amount of new urefs to be provisioned by host
    /// * `existing_urefs_ptr` - serialized list of existing [`casper_types::URef`]s
    /// * `existing_urefs_size` - size of serialized list of  [`casper_types::URef`]s
    /// * `output_size_ptr` - pointer to a value where a size of list of [`casper_types::URef`]s
    ///   written to host buffer will be set.
    pub fn casper_create_contract_user_group(
        contract_package_hash_ptr: *const u8,
        contract_package_hash_size: usize,
        label_ptr: *const u8,
        label_size: usize,
        num_new_urefs: u8,
        existing_urefs_ptr: *const u8,
        existing_urefs_size: usize,
        output_size_ptr: *mut usize,
    ) -> i32;
    /// Adds new contract version to a contract package.
    ///
    /// # Arguments
    ///
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `version_ptr` - output parameter where new version assigned by host is set
    /// * `entry_points_ptr` - pointer to serialized [`casper_types::EntryPoints`]
    /// * `entry_points_size` - size of serialized [`casper_types::EntryPoints`]
    /// * `named_keys_ptr` - pointer to serialized [`casper_types::contracts::NamedKeys`]
    /// * `named_keys_size` - size of serialized [`casper_types::contracts::NamedKeys`]
    /// * `output_ptr` - pointer to a memory where host assigned contract hash is set to
    /// * `output_size` - size of memory area that host can write to
    /// * `bytes_written_ptr` - pointer to a value where host will set a number of bytes written to
    ///   the `output_size` pointer
    pub fn casper_add_contract_version(
        contract_package_hash_ptr: *const u8,
        contract_package_hash_size: usize,
        version_ptr: *const u32,
        entry_points_ptr: *const u8,
        entry_points_size: usize,
        named_keys_ptr: *const u8,
        named_keys_size: usize,
        output_ptr: *mut u8,
        output_size: usize,
        bytes_written_ptr: *mut usize,
    ) -> i32;
    /// Disables contract in a contract package. Returns non-zero standard error for a failure,
    /// otherwise a zero indicates success.
    ///
    /// # Arguments
    ///
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `contract_hash_ptr` - pointer to serialized contract hash.
    /// * `contract_hash_size` - size of contract hash in serialized form.
    pub fn casper_disable_contract_version(
        contract_package_hash_ptr: *const u8,
        contract_package_hash_size: usize,
        contract_hash_ptr: *const u8,
        contract_hash_size: usize,
    ) -> i32;
    /// Calls a contract by its hash. Requires entry point name that has to be present on a
    /// specified contract, and serialized named arguments. Returns a standard error code in
    /// case of failure, otherwise a successful execution returns zero. Bytes returned from contract
    /// execution are set to `result_size` pointer.
    ///
    /// # Arguments
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `entry_point_name_ptr` - pointer to serialized contract entry point name
    /// * `entry_point_name_size` - size of serialized contract entry point name
    /// * `runtime_args_ptr` - pointer to serialized runtime arguments
    /// * `runtime_args_size` - size of serialized runtime arguments
    /// * `result_size` - a pointer to a value which will be set to a size of bytes of called
    ///   contract return value
    pub fn casper_call_contract(
        contract_hash_ptr: *const u8,
        contract_hash_size: usize,
        entry_point_name_ptr: *const u8,
        entry_point_name_size: usize,
        runtime_args_ptr: *const u8,
        runtime_args_size: usize,
        result_size: *mut usize,
    ) -> i32;
    /// Calls a contract by its package hash. Optionally accepts a serialized `Option<u32>` as a
    /// version that for `None` case would call most recent version for given protocol version,
    /// otherwise it selects a specific contract version. Requires an entry point name
    /// registered in a given version of contract. Returns a standard error code in case of
    /// failure, otherwise a successful execution returns zero. Bytes returned from contract
    /// execution are set to `result_size` pointer
    ///
    /// # Arguments
    ///
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `contract_version_ptr` - Contract package hash in a serialized form
    /// * `contract_version_size` -
    /// * `entry_point_name_ptr` -
    /// * `entry_point_name_size` -
    /// * `runtime_args_ptr` -
    /// * `runtime_args_size` -
    /// * `result_size` -
    pub fn casper_call_versioned_contract(
        contract_package_hash_ptr: *const u8,
        contract_package_hash_size: usize,
        contract_version_ptr: *const u8,
        contract_version_size: usize,
        entry_point_name_ptr: *const u8,
        entry_point_name_size: usize,
        runtime_args_ptr: *const u8,
        runtime_args_size: usize,
        result_size: *mut usize,
    ) -> i32;
    /// This function queries the host side to check for given named argument existence and returns
    /// a size in bytes of given argument. Returns zero for success or non-zero value for
    /// failure as described in standard error codes.
    ///
    /// # Arguments
    ///
    /// * `name_ptr` - pointer (offset in wasm memory) to the location where serialized argument
    ///   name is present
    /// * `name_size` - size of serialized bytes of argument name
    /// * `dest_ptr` - pointer to the location where argument bytes will be copied from the host
    ///   side
    /// * `dest_size` - size of destination pointer
    pub fn casper_get_named_arg_size(
        name_ptr: *const u8,
        name_size: usize,
        dest_size: *mut usize,
    ) -> i32;
    /// This function copies the contents of the current runtime buffer into the
    /// wasm memory, beginning at the provided offset. It is intended that this
    /// function be called after a call to `load_arg`. It is up to the caller to
    /// ensure that the proper amount of memory is allocated for this write,
    /// otherwise data corruption in the wasm memory may occur due to this call
    /// overwriting some bytes unintentionally. The size of the data which will be
    /// written is returned from the `load_arg` call. The bytes which are written
    /// are the those corresponding to the provided argument; it is up to the
    /// developer to know how to attempt to interpret those bytes.
    ///
    /// # Arguments
    ///
    /// * `name_ptr` - pointer (offset in wasm memory) to the location where serialized argument
    ///   name is present
    /// * `name_size` - size of serialized bytes of argument name
    /// * `dest_ptr` - pointer to the location where argument bytes will be copied from the host
    ///   side
    /// * `dest_size` - size of destination pointer
    pub fn casper_get_named_arg(
        name_ptr: *const u8,
        name_size: usize,
        dest_ptr: *mut u8,
        dest_size: usize,
    ) -> i32;
    /// Removes group from given contract package.
    ///
    /// # Arguments
    ///
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `label_ptr` - serialized group label
    /// * `label_size` - size of serialized group label
    pub fn casper_remove_contract_user_group(
        contract_package_hash_ptr: *const u8,
        contract_package_hash_size: usize,
        label_ptr: *const u8,
        label_size: usize,
    ) -> i32;
    /// Requests host to provision additional [`casper_types::URef`] to a specified group
    /// identified by its label. Returns standard error code for non-zero value, otherwise zero
    /// indicated success.
    ///
    /// # Arguments
    ///
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `label_ptr` - serialized group label
    /// * `label_size` - size of serialized group label
    /// * `value_size_ptr` - size of data written to a host buffer will be saved here
    pub fn casper_provision_contract_user_group_uref(
        contract_package_hash_ptr: *const u8,
        contract_package_hash_size: usize,
        label_ptr: *const u8,
        label_size: usize,
        value_size_ptr: *const usize,
    ) -> i32;
    /// Removes user group urefs. Accepts a contract package hash, label name of a group, and a list
    /// of urefs that will be removed from the group.
    ///
    /// # Arguments
    ///
    /// * `contract_package_hash_ptr` - pointer to serialized contract package hash.
    /// * `contract_package_hash_size` - size of contract package hash in serialized form.
    /// * `label_ptr` - serialized group label
    /// * `label_size` - size of serialized group label
    /// * `urefs_ptr` - pointer to serialized list of urefs
    /// * `urefs_size` - size of serialized list of urefs
    pub fn casper_remove_contract_user_group_urefs(
        contract_package_hash_ptr: *const u8,
        contract_package_hash_size: usize,
        label_ptr: *const u8,
        label_size: usize,
        urefs_ptr: *const u8,
        urefs_size: usize,
    ) -> i32;
    /// Returns a 32-byte BLAKE2b hash digest from the given input bytes
    ///
    /// # Arguments
    /// * `in_ptr` - pointer to bytes
    /// * `in_size` - length of bytes
    /// * `out_ptr` - pointer to the location where argument bytes will be copied from the host side
    /// * `out_size` - size of output pointer
    pub fn casper_blake2b(
        in_ptr: *const u8,
        in_size: usize,
        out_ptr: *mut u8,
        out_size: usize,
    ) -> i32;
    ///
    pub fn casper_load_call_stack(
        call_stack_len_ptr: *mut usize,
        result_size_ptr: *mut usize,
    ) -> i32;
    /// Prints data directly to standard output on the host.
    ///
    /// # Arguments
    ///
    /// * `text_ptr` - pointer to serialized text to print
    /// * `text_size` - size of serialized text to print
    #[cfg(feature = "test-support")]
    pub fn casper_print(text_ptr: *const u8, text_size: usize);
    /// Creates new URef that points to a dictionary partition of global state.
    ///
    /// # Arguments
    ///
    /// * `output_size` - pointer to a value where host will write size of bytes of created URef.
    pub fn casper_new_dictionary(output_size_ptr: *mut usize) -> i32;
    /// The bytes in wasm memory from offset `key_ptr` to `key_ptr + key_size`
    /// will be used together with the current context’s seed to form a dictionary.
    /// The value at that dictionary is read from the global state, serialized and
    /// buffered in the runtime. This result can be obtained via the [`casper_read_host_buffer`]
    /// function.
    ///
    /// # Arguments
    ///
    /// * `uref_ptr` - pointer to bytes representing the user-defined key
    /// * `uref_size` - size of the key (in bytes)
    /// * `key_bytes_ptr` - pointer to bytes representing the user-defined key
    /// * `key_bytes_size` - size of the user-defined key
    /// * `output_size` - pointer to a value where host will write size of bytes read from given key
    pub fn casper_dictionary_get(
        uref_ptr: *const u8,
        uref_size: usize,
        key_bytes_ptr: *const u8,
        key_bytes_size: usize,
        output_size: *mut usize,
    ) -> i32;
    /// The bytes in wasm memory from offset `key_ptr` to `key_ptr + key_size`
    /// will be used together with the passed URef's seed to form a dictionary.
    /// This function writes the provided value (read via de-serializing the bytes
    /// in wasm memory from offset `value_ptr` to `value_ptr + value_size`) under
    /// that dictionary in the global state. This function will cause a `Trap` if
    /// the value fails to de-serialize.
    ///
    /// # Arguments
    ///
    /// * `uref_ptr` - pointer to bytes representing the user-defined key
    /// * `uref_size` - size of the key (in bytes)
    /// * `key_ptr` - pointer to bytes representing the user-defined key to write to
    /// * `key_size` - size of the key (in bytes)
    /// * `value_ptr` - pointer to bytes representing the value to write at the key
    /// * `value_size` - size of the value (in bytes)
    pub fn casper_dictionary_put(
        uref_ptr: *const u8,
        uref_size: usize,
        key_ptr: *const u8,
        key_size: usize,
        value_ptr: *const u8,
        value_size: usize,
    ) -> i32;
}
