import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {CLValue} from "../../../../contract_as/assembly/clvalue";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {createPurse, transferFromPurseToPurse} from "../../../../contract_as/assembly/purse";
import {RuntimeArgs} from "../../../../contract_as/assembly/runtime_args";
import {Pair} from "../../../../contract_as/assembly/pair";

const POS_ACTION = "bond";
const ARG_AMOUNT = "amount";
const ARG_PURSE = "purse";

export function call(): void {
    let proofOfStake = CL.getSystemContract(CL.SystemContract.ProofOfStake);
    let mainPurse = getMainPurse();
    let bondingPurse = createPurse();

    let amountBytes = CL.getNamedArg(ARG_AMOUNT);
    if (amountBytes === null) {
        Error.fromErrorCode(ErrorCode.MissingArgument).revert();
        return;
    }

    let amountResult = U512.fromBytes(amountBytes);
    if (amountResult.hasError()) {
        Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
        return;
    }

    let amount = amountResult.value;

    let ret = transferFromPurseToPurse(
        mainPurse,
        bondingPurse,
        amount,
    );
    if (ret > 0) {
        Error.fromErrorCode(ErrorCode.Transfer).revert();
        return;
    }

    let bondingPurseValue = CLValue.fromURef(bondingPurse);
    let runtimeArgs = RuntimeArgs.fromArray([
        new Pair(ARG_AMOUNT, CLValue.fromU512(amount)),
        new Pair(ARG_PURSE, bondingPurseValue),
    ]);
    CL.callContract(proofOfStake, POS_ACTION, runtimeArgs);
}
