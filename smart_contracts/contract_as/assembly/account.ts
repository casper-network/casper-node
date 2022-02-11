import * as externals from "./externals";
import {arrayToTyped} from "./utils";
import {UREF_SERIALIZED_LENGTH} from "./constants";
import {URef} from "./uref";
import {AccountHash} from "./key";
import { fromBytesArray } from "./bytesrepr";
import {Error, ErrorCode} from "./error";
import {readHostBuffer} from "./index";

/**
 * Enum representing the possible results of adding an associated key to an account.
 */
export enum AddKeyFailure {
    /**
     * Success
     */
    Ok = 0,
    /**
     * Unable to add new associated key because maximum amount of keys is reached
     */
    MaxKeysLimit = 1,
    /**
     * Unable to add new associated key because given key already exists
     */
    DuplicateKey = 2,
    /**
     * Unable to add new associated key due to insufficient permissions
     */
    PermissionDenied = 3,
}

/**
 * Enum representing the possible results of updating an associated key of an account.
 */
export enum UpdateKeyFailure {
    /**
     * Success
     */
    Ok = 0,
    /**
     * Key does not exist in the list of associated keys.
     */
    MissingKey = 1,
    /**
     * Unable to update the associated key due to insufficient permissions
     */
    PermissionDenied = 2,
    /**
     * Unable to update weight that would fall below any of action thresholds
     */
    ThresholdViolation = 3,
}

/**
 * Enum representing the possible results of removing an associated key from an account.
 */
export enum RemoveKeyFailure {
    /**
     * Success
     */
    Ok = 0,
    /**
     * Key does not exist in the list of associated keys.
     */
    MissingKey = 1,
    /**
     * Unable to remove the associated key due to insufficient permissions
     */
    PermissionDenied = 2,
    /**
     * Unable to remove a key which would violate action threshold constraints
     */
    ThresholdViolation = 3,
}

/**
 * Enum representing the possible results of setting the threshold of an account.
 */
export enum SetThresholdFailure {
    /**
     * Success
     */
    Ok = 0,
    /**
     * New threshold should be lower or equal than deployment threshold
     */
    KeyManagementThreshold = 1,
    /**
     * New threshold should be lower or equal than key management threshold
     */
    DeploymentThreshold = 2,
    /**
     * Unable to set action threshold due to insufficient permissions
     */
    PermissionDeniedError = 3,
    /**
     * New threshold should be lower or equal than total weight of associated keys
     */
    InsufficientTotalWeight = 4,
}

/**
 * Enum representing an action for which a threshold is being set.
 */
export enum ActionType {
    /**
     * Required by deploy execution.
     */
    Deployment = 0,
    /**
     * Required when adding/removing associated keys, changing threshold levels.
     */
    KeyManagement = 1,
}

/**
 * Adds an associated key to the account. Associated keys are the keys allowed to sign actions performed
 * in the context of the account.
 *
 * @param AccountHash The public key to be added as the associated key.
 * @param weight The weight that will be assigned to the new associated key. See [[setActionThreshold]]
 *    for more info about weights.
 * @returns An instance of [[AddKeyFailure]] representing the result.
 */
export function addAssociatedKey(accountHash: AccountHash, weight: i32): AddKeyFailure {
    const accountHashBytes = accountHash.toBytes();
    const ret = externals.add_associated_key(accountHashBytes.dataStart, accountHashBytes.length, weight);
    return <AddKeyFailure>ret;
}

/**
 * Sets a threshold for the action performed in the context of the account.
 *
 * Each request has to be signed by one or more of the keys associated with the account. The action
 * is only successful if the total weights of the signing associated keys is greater than the threshold.
 *
 * @param actionType The type of the action for which the threshold is being set.
 * @param thresholdValue The minimum total weight of the keys of the action to be successful.
 * @returns An instance of [[SetThresholdFailure]] representing the result.
 */
export function setActionThreshold(actionType: ActionType, thresholdValue: u8): SetThresholdFailure {
    const ret = externals.set_action_threshold(<i32>actionType, thresholdValue);
    return <SetThresholdFailure>ret;
}

/**
 * Changes the weight of an existing associated key. See [[addAssociatedKey]] and [[setActionThreshold]]
 * for info about associated keys and their weights.
 *
 * @param accountHash The associated key to be updated.
 * @param weight The new desired weight of the associated key.
 * @returns An instance of [[UpdateKeyFailure]] representing the result.
 */
export function updateAssociatedKey(accountHash: AccountHash, weight: i32): UpdateKeyFailure {
    const accountHashBytes = accountHash.toBytes();
    const ret = externals.update_associated_key(accountHashBytes.dataStart, accountHashBytes.length, weight);
    return <UpdateKeyFailure>ret;
}

/**
 * Removes the associated key from the account. See [[addAssociatedKey]] for more info about associated
 * keys.
 *
 * @param accountHash The associated key to be removed.
 * @returns An instance of [[RemoveKeyFailure]] representing the result.
 */
export function removeAssociatedKey(accountHash: AccountHash): RemoveKeyFailure {
    const accountHashBytes = accountHash.toBytes();
    const ret = externals.remove_associated_key(accountHashBytes.dataStart, accountHashBytes.length);
    return <RemoveKeyFailure>ret;
}

/**
 * Gets the [[URef]] representing the main purse of the account.
 *
 * @returns The [[URef]] that can be used to access the main purse.
 */
export function getMainPurse(): URef {
    let data = new Uint8Array(UREF_SERIALIZED_LENGTH);
    data.fill(0);
    externals.get_main_purse(data.dataStart);
    let urefResult = URef.fromBytes(data);
    return urefResult.unwrap();
}

/**
 * Returns the set of [[AccountHash]] from the calling account's context `authorization_keys`.
 */
export function listAuthorizationKeys(): Array<AccountHash> {
    let totalKeys = new Uint32Array(1);
    let resultSize = new Uint32Array(1);

    const res = externals.load_authorization_keys(totalKeys.dataStart, resultSize.dataStart);
    const error = Error.fromResult(res);
    if (error !== null) {
        error.revert();
        unreachable();
    }

    if (totalKeys[0] == 0) {
        return new Array<AccountHash>();
    }

    let keyBytes = readHostBuffer(resultSize[0]);
    let maybeKeys = fromBytesArray<AccountHash>(
        keyBytes,
        AccountHash.fromBytes);

    if (maybeKeys.hasError()) {
      Error.fromErrorCode(ErrorCode.Deserialize).revert();
      unreachable();
    }

    let result = maybeKeys.value;
    result.sort();
    return result;
}
