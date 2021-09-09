//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {Key} from "../../../../contract_as/assembly/key";
import {putKey} from "../../../../contract_as/assembly";
import {CLValue} from "../../../../contract_as/assembly/clvalue";
import {getPurseBalance, transferFromPurseToPublicKey, TransferredTo} from "../../../../contract_as/assembly/purse";
import {URef} from "../../../../contract_as/assembly/uref";
import {PublicKey} from "../../../../contract_as/assembly/public_key";

const ARG_SOURCE_PURSE = "source_purse";
const ARG_TARGET = "target";
const ARG_AMOUNT = "amount";

export function delegate(): void {
    const sourcePurseArg = CL.getNamedArg(ARG_SOURCE_PURSE);
    const sourcePurse = URef.fromBytes(sourcePurseArg).unwrap();
    
    const targetArg = CL.getNamedArg(ARG_TARGET);
    const target = PublicKey.fromBytes(targetArg).unwrap();

    const amountArg = CL.getNamedArg(ARG_AMOUNT);
    const amount = U512.fromBytes(amountArg).unwrap();

    const result = transferFromPurseToPublicKey(<URef>sourcePurse, <PublicKey>target, amount);
    if (result.isErr) {
        Error.fromErrorCode(ErrorCode.Transfer).revert();
    }
}

export function call(): void {
    delegate();
}
