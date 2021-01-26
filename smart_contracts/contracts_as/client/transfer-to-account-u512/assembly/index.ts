import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {transferToAccount, TransferredTo} from "../../../../contract_as/assembly/purse";

const ARG_TARGET = "target";
const ARG_AMOUNT = "amount";


export function call(): void {
    const accountBytes = CL.getNamedArg(ARG_TARGET);
    const amountBytes = CL.getNamedArg(ARG_AMOUNT);
    const amountResult = U512.fromBytes(amountBytes);
    if (amountResult.hasError()){
        Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
        return;
    }
    let amount = amountResult.value;

    const result = transferToAccount(accountBytes, amount);
    if (result.isErr) {
        result.err.revert();
        return;
    }
}
