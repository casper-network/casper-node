//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error } from "../../../../contract_as/assembly/error";
import { U512 } from "../../../../contract_as/assembly/bignum";
import { getMainPurse } from "../../../../contract_as/assembly/account";
import { Key } from "../../../../contract_as/assembly/key";
import { getKey, hasKey, putKey } from "../../../../contract_as/assembly";
import { CLValue } from "../../../../contract_as/assembly/clvalue";
import { fromBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { URef } from "../../../../contract_as/assembly/uref";
import { createPurse, getPurseBalance, transferFromPurseToPurse } from "../../../../contract_as/assembly/purse";

const PURSE_MAIN = "purse:main";
const PURSE_TRANSFER_RESULT = "purse_transfer_result";
const MAIN_PURSE_BALANCE = "main_purse_balance";
const SUCCESS_MESSAGE = "Ok(())";
const TRANSFER_ERROR_MESSAGE = "Err(ApiError::Mint(InsufficientFunds) [65024])";

const ARG_SOURCE = "source";
const ARG_TARGET = "target";
const ARG_AMOUNT = "amount";

enum CustomError {
    MissingSourcePurseArg = 1,
    InvalidSourcePurseArg = 2,
    MissingDestinationPurseArg = 3,
    InvalidDestinationPurseArg = 4,
    MissingDestinationPurse = 5,
    UnableToStoreResult = 6,
    UnableToStoreBalance = 7,
    MissingAmountArg = 8,
    InvalidAmountArg = 9,
    InvalidSourcePurseKey = 103,
    UnexpectedSourcePurseKeyVariant = 104,
    InvalidDestinationPurseKey = 105,
    UnexpectedDestinationPurseKeyVariant = 106,
    UnableToGetBalance = 107,
}

export function call(): void {
    const mainPurse = getMainPurse();
    const mainPurseKey = Key.fromURef(mainPurse);
    putKey(PURSE_MAIN, mainPurseKey);
    const sourcePurseKeyNameArg = CL.getNamedArg(ARG_SOURCE);
    const maybeSourcePurseKeyName = fromBytesString(sourcePurseKeyNameArg);
    if (maybeSourcePurseKeyName.hasError()) {
        Error.fromUserError(<u16>CustomError.InvalidSourcePurseArg).revert();
        return;
    }
    const sourcePurseKeyName = maybeSourcePurseKeyName.value;
    const sourcePurseKey = getKey(sourcePurseKeyName);
    if (!sourcePurseKey) {
        Error.fromUserError(<u16>CustomError.InvalidSourcePurseKey).revert();
        return;
    }
    if (!sourcePurseKey.isURef()) {
        Error.fromUserError(<u16>CustomError.UnexpectedSourcePurseKeyVariant).revert();
        return;
    }
    const sourcePurse = sourcePurseKey.toURef();

    const destinationPurseKeyNameArg = CL.getNamedArg(ARG_TARGET);
    if (!destinationPurseKeyNameArg) {
        Error.fromUserError(<u16>CustomError.MissingDestinationPurseArg).revert();
        return;
    }
    const maybeDestinationPurseKeyName = fromBytesString(destinationPurseKeyNameArg);
    if (maybeDestinationPurseKeyName.hasError()) {
        Error.fromUserError(<u16>CustomError.InvalidDestinationPurseArg).revert();
        return;
    }
    let destinationPurseKeyName = maybeDestinationPurseKeyName.value;
    let destinationPurse: URef | null;
    let destinationKey: Key | null;
    if (!hasKey(destinationPurseKeyName)) {
        destinationPurse = createPurse();
        destinationKey = Key.fromURef(destinationPurse);
        putKey(destinationPurseKeyName, destinationKey);
    } else {
        destinationKey = getKey(destinationPurseKeyName);
        if (!destinationKey) {
            Error.fromUserError(<u16>CustomError.InvalidDestinationPurseKey).revert();
            return;
        }
        if (!destinationKey.isURef()) {
            Error.fromUserError(<u16>CustomError.UnexpectedDestinationPurseKeyVariant).revert();
            return;
        }
        destinationPurse = destinationKey.toURef();
    }
    if (!destinationPurse) {
        Error.fromUserError(<u16>CustomError.MissingDestinationPurse).revert();
        return;
    }

    const amountArg = CL.getNamedArg(ARG_AMOUNT);
    const amountResult = U512.fromBytes(amountArg);
    if (amountResult.hasError()) {
        Error.fromUserError(<u16>CustomError.InvalidAmountArg).revert();
        return;
    }
    const amount = amountResult.value;

    const error = transferFromPurseToPurse(<URef>sourcePurse, <URef>destinationPurse, amount);
    let message = SUCCESS_MESSAGE;
    if (error !== null) {
        message = TRANSFER_ERROR_MESSAGE;
    }
    const resultKey = Key.create(CLValue.fromString(message));
    const finalBalance = getPurseBalance(<URef>sourcePurse);
    if (!finalBalance) {
        Error.fromUserError(<u16>CustomError.UnableToGetBalance).revert();
        return;
    }
    const balanceKey = Key.create(CLValue.fromU512(finalBalance));
    if (!balanceKey) {
        Error.fromUserError(<u16>CustomError.UnableToStoreBalance).revert();
        return;
    }
    if (!resultKey) {
        Error.fromUserError(<u16>CustomError.UnableToStoreResult).revert();
        return;
    }
    putKey(PURSE_TRANSFER_RESULT, <Key>resultKey);
    putKey(MAIN_PURSE_BALANCE, <Key>balanceKey);
}
