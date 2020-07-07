import * as CL from "../../../../contract-as/assembly";
import {Error, ErrorCode} from "../../../../contract-as/assembly/error";
import {U512} from "../../../../contract-as/assembly/bignum";
import {CLValue} from "../../../../contract-as/assembly/clvalue";
import {RuntimeArgs} from "../../../../contract-as/assembly/runtime_args";
import {Pair} from "../../../../contract-as/assembly/pair";

const POS_ACTION = "unbond";
const ARG_AMOUNT = "amount";

export function call(): void {
    let proofOfStake = CL.getSystemContract(CL.SystemContract.ProofOfStake);

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
    let runtimeArgs = RuntimeArgs.fromArray([
        new Pair(ARG_AMOUNT, CLValue.fromU512(amount)),
    ]);
    CL.callContract(proofOfStake, POS_ACTION, runtimeArgs);
}
