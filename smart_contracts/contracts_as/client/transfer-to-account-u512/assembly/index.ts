import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {transferFromPurseToAccount, TransferredTo} from "../../../../contract_as/assembly/purse";

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
    const mainPurse = getMainPurse();

    const result = transferFromPurseToAccount(mainPurse, accountBytes, amount);
    if (result == TransferredTo.TransferError){
        Error.fromErrorCode(ErrorCode.Transfer).revert();
        return;
    }
}
