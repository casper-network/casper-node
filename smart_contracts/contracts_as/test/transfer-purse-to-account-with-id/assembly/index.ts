//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { U512 } from "../../../../contract_as/assembly/bignum";
import { getMainPurse } from "../../../../contract_as/assembly/account";
import { Key } from "../../../../contract_as/assembly/key";
import { putKey } from "../../../../contract_as/assembly";
import { CLValue } from "../../../../contract_as/assembly/clvalue";
import { getPurseBalance, transferFromPurseToAccount, TransferredTo } from "../../../../contract_as/assembly/purse";
import { URef } from "../../../../contract_as/assembly/uref";
import { Option } from "../../../../contract_as/assembly/option";
import { fromBytesU64 } from "../../../../contract_as/assembly/bytesrepr";
import { Ref } from "../../../../contract_as/assembly/ref";


const TRANSFER_RESULT_UREF_NAME = "transfer_result";
const MAIN_PURSE_FINAL_BALANCE_UREF_NAME = "final_balance";

const ARG_TARGET = "target";
const ARG_AMOUNT = "amount";
const ARG_ID = "id";

enum CustomError {
    MissingAmountArg = 1,
    InvalidAmountArg = 2,
    MissingDestinationAccountArg = 3,
    UnableToGetBalance = 103
}

export function delegate(): void {
    const mainPurse = getMainPurse();
    const destinationAccountAddrArg = CL.getNamedArg(ARG_TARGET);
    const amountArg = CL.getNamedArg(ARG_AMOUNT);
    const amountResult = U512.fromBytes(amountArg);
    const idBytes = CL.getNamedArg(ARG_ID);
    const maybeOptionalId = Option.fromBytes(idBytes);

    let maybeId: Ref<u64> | null = null;
    if (maybeOptionalId.isSome()) {
        const maybeIdBytes = maybeOptionalId.unwrap();
        maybeId = new Ref(fromBytesU64(StaticArray.fromArray(maybeIdBytes)).unwrap());
    }

    if (amountResult.hasError()) {
        Error.fromUserError(<u16>CustomError.InvalidAmountArg).revert();
        return;
    }
    let amount = amountResult.value;
    const result = transferFromPurseToAccount(<URef>mainPurse, destinationAccountAddrArg, amount, maybeId);
    let message = "";
    if (result.isOk) {
        const foo = result.ok;
        switch (result.ok) {
            case TransferredTo.NewAccount:
                message = "Ok(NewAccount)";
                break;
            case TransferredTo.ExistingAccount:
                message = "Ok(ExistingAccount)";
                break;
        }
    }

    if (result.isErr) {
        message = "Err(ApiError::Mint(InsufficientFunds) [65024])";
    }
    const transferResultKey = Key.create(CLValue.fromString(message));
    putKey(TRANSFER_RESULT_UREF_NAME, <Key>transferResultKey);
    const maybeBalance = getPurseBalance(mainPurse);
    if (!maybeBalance) {
        Error.fromUserError(<u16>CustomError.UnableToGetBalance).revert();
        return;
    }
    const balanceKey = Key.create(CLValue.fromU512(<U512>maybeBalance));
    putKey(MAIN_PURSE_FINAL_BALANCE_UREF_NAME, <Key>balanceKey);
}

export function call(): void {
    delegate();
}
