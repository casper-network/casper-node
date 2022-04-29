//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {Key} from "../../../../contract_as/assembly/key";
import {putKey} from "../../../../contract_as/assembly";
import {CLValue} from "../../../../contract_as/assembly/clvalue";
import {getPurseBalance, transferFromPurseToAccount, TransferredTo} from "../../../../contract_as/assembly/purse";
import {URef} from "../../../../contract_as/assembly/uref";


const TRANSFER_RESULT_UREF_NAME = "transfer_result";
const MAIN_PURSE_FINAL_BALANCE_UREF_NAME = "final_balance";

const ARG_TARGET = "target";
const ARG_AMOUNT = "amount";

enum CustomError{
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
    if (amountResult.hasError()) {
        Error.fromUserError(<u16>CustomError.InvalidAmountArg).revert();
        return;
    }
    let amount = amountResult.value;
    let message = "";
    const result = transferFromPurseToAccount(<URef>mainPurse, <Uint8Array>destinationAccountAddrArg, amount);
    if (result.isErr) {
        let error = result.err;
        error.revert();
    }
}

export function call(): void {
    delegate();
}
