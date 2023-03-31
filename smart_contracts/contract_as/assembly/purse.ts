import * as externals from "./externals";
import {readHostBuffer} from "./index";
import {U512} from "./bignum";
import {Error, ErrorCode} from "./error";
import {UREF_SERIALIZED_LENGTH} from "./constants";
import {URef} from "./uref";
import {toBytesU64} from "./bytesrepr";
import {Option} from "./option";
import {Ref} from "./ref";
import {getMainPurse} from "./account";
import {arrayToTyped} from "./utils";
import {PublicKey} from "./public_key";
import {AccountHash} from "./key";

/**
 * The result of a successful transfer between purses.
 */
export enum TransferredTo {
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
 * The result of a transfer between purse and account.
 */
export class TransferResult {
    public errValue: Error | null = null;
    public okValue: Ref<TransferredTo> | null = null;

    static makeErr(err: Error): TransferResult {
        let transferResult = new TransferResult();
        transferResult.errValue = err;
        return transferResult;
    }

    static makeOk(ok: Ref<TransferredTo>): TransferResult {
        let transferResult = new TransferResult();
        transferResult.okValue = ok;
        return transferResult;
    }

    get isErr(): bool {
        return this.errValue !== null;
    }

    get isOk(): bool {
        return this.okValue !== null;
    }

    get ok(): TransferredTo {
        assert(this.okValue !== null);
        const ok = <Ref<i32>>this.okValue;
        return ok.value;
    }

    get err(): Error {
        assert(this.errValue !== null);
        return <Error>this.errValue;
    }
}

function makeTransferredTo(value: u32): Ref<TransferredTo> | null {
    if (value == <u32>TransferredTo.ExistingAccount)
        return new Ref(TransferredTo.ExistingAccount);
    if (value == <u32>TransferredTo.NewAccount)
        return new Ref(TransferredTo.NewAccount);
    return null;
}

/**
 * Creates a new empty purse and returns its [[URef]], or a null in case a
 * purse couldn't be created.
 */
export function createPurse(): URef {
    let bytes = new StaticArray<u8>(UREF_SERIALIZED_LENGTH);
    let ret = externals.create_purse(
        changetype<usize>(bytes),
        bytes.length
        );
    let error = Error.fromResult(<u32>ret);
    if (error !== null){
        error.revert();
        unreachable();
    }

    let urefResult = URef.fromBytes(bytes);
    if (urefResult.hasError()) {
        Error.fromErrorCode(ErrorCode.PurseNotCreated).revert();
        unreachable();
    }

    return urefResult.value;
}

/**
 * Returns the balance in motes of the given purse or a null if given purse
 * is invalid.
 * @hidden
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
        unreachable();
    }

    let balanceBytes = readHostBuffer(balanceSize[0]);
    let balanceResult = U512.fromBytes(balanceBytes);
    return balanceResult.unwrap();
}

export function getBalance(): U512 | null {
    getPurseBalance(getMainPurse())
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
export function transferFromPurseToAccount(sourcePurse: URef, targetAccount: StaticArray<u8>, amount: U512, id: Ref<u64> | null = null): TransferResult {
    let purseBytes = sourcePurse.toBytes();
    let amountBytes = amount.toBytes();

    let optId: Option;
    if (id !== null) {
        optId = new Option(toBytesU64(id.value));
    }
    else {
        optId = new Option(null);
    }
    const idBytes = optId.toBytes();

    let resultPtr = new Uint32Array(1);

    let ret = externals.transfer_from_purse_to_account(
        purseBytes.dataStart,
        purseBytes.length,
        changetype<usize>(targetAccount),
        targetAccount.length,
        amountBytes.dataStart,
        amountBytes.length,
        idBytes.dataStart,
        idBytes.length,
        resultPtr.dataStart,
    );

    const error = Error.fromResult(ret);
    if (error !== null) {
        return TransferResult.makeErr(error);
    }

    const transferredTo = makeTransferredTo(resultPtr[0]);
    if (transferredTo !== null) {
        return TransferResult.makeOk(transferredTo);
    }
    return TransferResult.makeErr(Error.fromErrorCode(ErrorCode.Transfer));
}

/**
 * Transfers `amount` of motes from `source` purse to `target` purse.  If `target` does not exist
 * the transfer fails.
 *
 * @returns This function returns non-zero value on error.
 */
export function transferFromPurseToPurse(sourcePurse: URef, targetPurse: URef, amount: U512, id: Ref<u64> | null = null): Error | null {
    let sourceBytes = sourcePurse.toBytes();
    let targetBytes = targetPurse.toBytes();
    let amountBytes = amount.toBytes();

    let optId: Option;
    if (id !== null) {
        const idValue = (<Ref<u64>>id).value;
        optId = new Option(toBytesU64(idValue));
    }
    else {
        optId = new Option(null);
    }
    const idBytes = optId.toBytes();

    let ret = externals.transfer_from_purse_to_purse(
        sourceBytes.dataStart,
        sourceBytes.length,
        targetBytes.dataStart,
        targetBytes.length,
        amountBytes.dataStart,
        amountBytes.length,
        idBytes.dataStart,
        idBytes.length,
    );

    return Error.fromResult(ret);
}

/**
 * Transfers `amount` of motes from `source` purse to `target` purse.  If `target` does not exist
 * the transfer fails.
 *
 * This function assumes both `sourcePurse` and `targetPurse` is already serialized. This is
 * useful in scenarios where both `sourcePurse` and `targetPurse` are received through named
 * arguments, or obtained from `getKey` APIs and there is no need to deserialize and
 * serialize objects again.
 *
 * @internal
 * @returns This function returns non-zero value on error.
 */
 export function transferFromPurseToPursePassthrough(sourceBytes: StaticArray<u8>, targetBytes: StaticArray<u8>, amountBytes: StaticArray<u8>, id: Ref<u64> | null = null): Error | null {
    let optId: Option;
    if (id !== null) {
        const idValue = (<Ref<u64>>id).value;
        optId = new Option(toBytesU64(idValue));
    }
    else {
        optId = new Option(null);
    }
    const idBytes = optId.toBytes();

    let ret = externals.transfer_from_purse_to_purse(
        changetype<usize>(sourceBytes),
        sourceBytes.length,
        changetype<usize>(targetBytes),
        targetBytes.length,
        changetype<usize>(amountBytes),
        amountBytes.length,
        idBytes.dataStart,
        idBytes.length,
    );

    return Error.fromResult(ret);
}


/**
 * Transfers `amount` of motes from `source` purse to an account referenced by a `targetPublicKey` public key.
 * If `target` does not exist it will be created.
 *
 * @param amount Amount is denominated in motes
 * @returns This function will return a [[TransferredTo.TransferError]] in
 * case of transfer error, in case of any other variant the transfer itself
 * can be considered successful.
 * @hidden
 */
 export function transferFromPurseToPublicKey(sourcePurse: URef, targetPublicKey: PublicKey, amount: U512, id: Ref<u64> | null = null): TransferResult {
    const accountHash = AccountHash.fromPublicKey(targetPublicKey);
    const accountHashBytes = accountHash.toBytes();
    return transferFromPurseToAccount(sourcePurse, StaticArray.fromArray(accountHashBytes), amount, id);
 }

/**
 * Transfers `amount` of motes from main purse purse to `target` account.
 * If `target` does not exist it will be created.
 *
 * @param amount Amount is denominated in motes
 * @returns This function will return a [[TransferredTo.TransferError]] in
 * case of transfer error, in case of any other variant the transfer itself
 * can be considered successful.
 */
export function transferToAccount(targetAccount: StaticArray<u8>, amount: U512, id: Ref<u64> | null = null): TransferResult {
    let amountBytes = amount.toBytes();

    let optId: Option;
    if (id !== null) {
        optId = new Option(toBytesU64(id.value));
    }
    else {
        optId = new Option(null);
    }
    const idBytes = optId.toBytes();

    let resultPtr = new Uint32Array(1);

    let ret = externals.transfer_to_account(
        changetype<usize>(targetAccount),
        targetAccount.length,
        amountBytes.dataStart,
        amountBytes.length,
        idBytes.dataStart,
        idBytes.length,
        resultPtr.dataStart,
    );

    const error = Error.fromResult(ret);
    if (error !== null) {
        return TransferResult.makeErr(error);
    }

    const transferredTo = makeTransferredTo(resultPtr[0]);
    if (transferredTo !== null) {
        return TransferResult.makeOk(transferredTo);
    }
    return TransferResult.makeErr(Error.fromErrorCode(ErrorCode.Transfer));
}

/**
 * Transfers `amount` of motes from main purse to `target` public key.
 * If account referenced by a `target` public key does not exist it will be created.
 *
 * @param amount Amount is denominated in motes
 * @returns This function will return a [[TransferredTo.TransferError]] in
 * case of transfer error, in case of any other variant the transfer itself
 * can be considered successful.
 */
 export function transferToPublicKey(targetPublicKey: PublicKey, amount: U512, id: Ref<u64> | null = null): TransferResult {
    const accountHash = AccountHash.fromPublicKey(targetPublicKey);
    const accountHashBytes = accountHash.toBytes();

    // const targetAccount = arrayToTyped(accountHashBytes);
    return transferToAccount(StaticArray.fromArray(accountHashBytes), amount, id);
}
