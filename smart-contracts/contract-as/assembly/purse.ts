import * as externals from "./externals";
import {readHostBuffer} from "./index";
import {U512} from "./bignum";
import {Error, ErrorCode} from "./error";
import {UREF_SERIALIZED_LENGTH} from "./constants";
import {URef} from "./uref";

/**
 * The result of a successful transfer between purses.
 */
export enum TransferredTo {
    /**
     * The transfer operation resulted in an error.
     */
    TransferError = -1,
    /**
     * The destination account already existed.
     */
    ExistingAccount = 0,
    /**
     * The destination account was created.
     */
    NewAccount = 1,
}

/**
 * Creates a new empty purse and returns its [[URef]], or a null in case a
 * purse couldn't be created.
 */
export function createPurse(): URef {
    let bytes = new Uint8Array(UREF_SERIALIZED_LENGTH);
    let ret = externals.create_purse(
        bytes.dataStart,
        bytes.length
        );
    let error = Error.fromResult(<u32>ret);
    if (error !== null){
        error.revert();
        return <URef>unreachable();
    }

    let urefResult = URef.fromBytes(bytes);
    if (urefResult.hasError()) {
        Error.fromErrorCode(ErrorCode.PurseNotCreated).revert();
        return <URef>unreachable();
    }

    return urefResult.value;
}

/**
 * Returns the balance in motes of the given purse or a null if given purse
 * is invalid.
 */
export function getPurseBalance(purse: URef): U512 | null {
    let purseBytes = purse.toBytes();
    let balanceSize = new Array<u32>(1);
    balanceSize[0] = 0;

    let retBalance = externals.get_balance(
        purseBytes.dataStart,
        purseBytes.length,
        balanceSize.dataStart,
    );

    const error = Error.fromResult(retBalance);
    if (error != null) {
        if (error.value() == ErrorCode.InvalidPurse) {
            return null;
        }
        error.revert();
        return <U512>unreachable();
    }

    let balanceBytes = readHostBuffer(balanceSize[0]);
    let balanceResult = U512.fromBytes(balanceBytes);
    return balanceResult.unwrap();
}

/**
 * Transfers `amount` of motes from `source` purse to `target` account.
 * If `target` does not exist it will be created.
 *
 * @param amount Amount is denominated in motes
 * @returns This function will return a [[TransferredTo.TransferError]] in
 * case of transfer error, in case of any other variant the transfer itself
 * can be considered successful.
 */
export function transferFromPurseToAccount(sourcePurse: URef, targetAccount: Uint8Array, amount: U512): TransferredTo {
    let purseBytes = sourcePurse.toBytes();
    let targetBytes = new Array<u8>(targetAccount.length);
    for (let i = 0; i < targetAccount.length; i++) {
        targetBytes[i] = targetAccount[i];
    }

    let amountBytes = amount.toBytes();

    let ret = externals.transfer_from_purse_to_account(
        purseBytes.dataStart,
        purseBytes.length,
        targetBytes.dataStart,
        targetBytes.length,
        amountBytes.dataStart,
        amountBytes.length,
    );

    if (ret == TransferredTo.ExistingAccount)
        return TransferredTo.ExistingAccount;
    if (ret == TransferredTo.NewAccount)
        return TransferredTo.NewAccount;
    return TransferredTo.TransferError;
}

/**
 * Transfers `amount` of motes from `source` purse to `target` purse.  If `target` does not exist
 * the transfer fails.
 *
 * @returns This function returns non-zero value on error.
 */
export function transferFromPurseToPurse(sourcePurse: URef, targetPurse: URef, amount: U512): i32 {
    let sourceBytes = sourcePurse.toBytes();
    let targetBytes = targetPurse.toBytes();
    let amountBytes = amount.toBytes();

    let ret = externals.transfer_from_purse_to_purse(
        sourceBytes.dataStart,
        sourceBytes.length,
        targetBytes.dataStart,
        targetBytes.length,
        amountBytes.dataStart,
        amountBytes.length,
    );
    return ret;
}
