import * as externals from "./externals";

/**
 * Offset of a reserved range dedicated for system contract errors.
 * @internal
 */
const SYSTEM_CONTRACT_ERROR_CODE_OFFSET: u32 = 65024;

/**
 * Offset of user errors
 */
const USER_ERROR_CODE_OFFSET: u32 = 65535;

/**
 * Standard error codes which can be encountered while running a smart contract.
 *
 * An [[ErrorCode]] can be passed to [[Error.fromErrorCode]] function to create an error object.
 * This error object later can be used to stop execution by using [[Error.revert]] method.
 */
export const enum ErrorCode {
    /** Optional data was unexpectedly `None`. */
    None = 1,
    /** Specified argument not provided. */
    MissingArgument = 2,
    /** Argument not of correct type. */
    InvalidArgument = 3,
    /** Failed to deserialize a value. */
    Deserialize = 4,
    /** `casper_contract::storage::read()` returned an error. */
    Read = 5,
    /** The given key returned a `None` value. */
    ValueNotFound = 6,
    /** Failed to find a specified contract. */
    ContractNotFound = 7,
    /** A call to [[getKey]] returned a failure. */
    GetKey = 8,
    /** The [[Key]] variant was not as expected. */
    UnexpectedKeyVariant = 9,
    /** The `Contract` variant was not as expected. */
    UnexpectedContractRefVariant = 10,
    /** Invalid purse name given. */
    InvalidPurseName = 11,
    /** Invalid purse retrieved. */
    InvalidPurse = 12,
    /** Failed to upgrade contract at [[URef]]. */
    UpgradeContractAtURef = 13,
    /** Failed to transfer motes. */
    Transfer = 14,
    /** The given [[URef]] has no access rights. */
    NoAccessRights = 15,
    /** A given type could not be constructed from a [[CLValue]]. */
    CLTypeMismatch = 16,
    /** Early end of stream while deserializing. */
    EarlyEndOfStream = 17,
    /** Formatting error while deserializing. */
    Formatting = 18,
    /** Not all input bytes were consumed in deserializing operation */
    LeftOverBytes = 19,
    /** Out of memory error. */
    OutOfMemory = 20,
    /** There are already maximum public keys associated with the given account. */
    MaxKeysLimit = 21,
    /** The given public key is already associated with the given account. */
    DuplicateKey = 22,
    /** Caller doesn't have sufficient permissions to perform the given action. */
    PermissionDenied = 23,
    /** The given public key is not associated with the given account. */
    MissingKey = 24,
    /** Removing/updating the given associated public key would cause the total weight of all remaining `AccountHash`s to fall below one of the action thresholds for the given account. */
    ThresholdViolation = 25,
    /** Setting the key-management threshold to a value lower than the deployment threshold is disallowed. */
    KeyManagementThreshold = 26,
    /** Setting the deployment threshold to a value greater than any other threshold is disallowed. */
    DeploymentThreshold = 27,
    /** Setting a threshold to a value greater than the total weight of associated keys is disallowed. */
    InsufficientTotalWeight = 28,
    /** The given `u32` doesn't map to a [[SystemContractType]]. */
    InvalidSystemContract = 29,
    /** Failed to create a new purse. */
    PurseNotCreated = 30,
    /** An unhandled value, likely representing a bug in the code. */
    Unhandled = 31,
    /** The provided buffer is too small to complete an operation. */
    BufferTooSmall = 32,
    /** No data available in the host buffer. */
    HostBufferEmpty = 33,
    /** The host buffer has been set to a value and should be consumed first by a read operation. */
    HostBufferFull = 34,
}


/**
 * This class represents error condition and is constructed by passing an error value.
 *
 * The variants are split into numeric ranges as follows:
 *
 * | Inclusive range | Variant(s)                                   |
 * | ----------------| ---------------------------------------------|
 * | [1, 65023]      | all except `Mint`, `HandlePayment` and `User`. Can be created with [[Error.fromErrorCode]] |
 * | [65024, 65279]  | `Mint` - instantiation currently unsupported |
 * | [65280, 65535]  | `HandlePayment` errors |
 * | [65536, 131071] | User error codes created with [[Error.fromUserError]] |
 *
 * ## Example usage
 *
 * ```typescript
 * // Creating using user error which adds 65536 to the error value.
 * Error.fromUserError(1234).revert();
 *
 * // Creating using standard error variant.
 * Error.fromErrorCode(ErrorCode.InvalidArguent).revert();
 * ```
 */
export class Error {
    private errorCodeValue: u32;

    /**
     * Creates an error object with given error value.
     *
     * Recommended way to use this class is through its static members:
     *
     * * [[Error.fromUserCode]]
     * * [[Error.fromErrorCode]]
     * @param value Error value
     */
    constructor(value: u32) {
        this.errorCodeValue = value;
    }

    /**
     * Creates an error object from a result value.
     *
     * Results in host interface contains 0 for a successful operation,
     * or a non-zero standardized error otherwise.
     *
     * @param result A result value obtained from host interface functions.
     * @returns Error object with an error [[ErrorCode]] variant.
     */
    static fromResult(result: u32): Error | null {
        if (result == 0) {
            // Ok
            return null;
        }
        return new Error(result);
    }

    /**
     * Creates new error from user value.
     *
     * Actual value held by returned [[Error]] object will be 65536 with added passed value.
     * @param userErrorCodeValue
     */
    static fromUserError(userErrorCodeValue: u16): Error {
        return new Error(USER_ERROR_CODE_OFFSET + 1 + userErrorCodeValue);
    }

    /**
     * Creates new error object from an [[ErrorCode]] value.
     *
     * @param errorCode Variant of a standarized error.
     */
    static fromErrorCode(errorCode: ErrorCode): Error {
        return new Error(<u32>errorCode);
    }

    /**
     * Returns an error value.
     */
    value(): u32{
        return this.errorCodeValue;
    }

    /**
     * Checks if error value is contained within user error range.
     */
    isUserError(): bool{
        return this.errorCodeValue > USER_ERROR_CODE_OFFSET;
    }

    /**
     * Checks if error value is contained within system contract error range.
     */
    isSystemContractError(): bool{
        return this.errorCodeValue >= SYSTEM_CONTRACT_ERROR_CODE_OFFSET && this.errorCodeValue <= USER_ERROR_CODE_OFFSET;
    }

    /**
     * Reverts execution of current contract with an error value contained within this error instance.
     */
    revert(): void {
        externals.revert(this.errorCodeValue);
    }
}
