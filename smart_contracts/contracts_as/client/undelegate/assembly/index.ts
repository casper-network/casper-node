import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {CLType, CLTypeTag, CLValue} from "../../../../contract_as/assembly/clvalue";
import {RuntimeArgs} from "../../../../contract_as/assembly/runtime_args";
import {Pair} from "../../../../contract_as/assembly/pair";
import {PublicKey} from "../../../../contract_as/assembly/public_key";
import {Option} from "../../../../contract_as/assembly/option";

const ARG_AMOUNT = "amount";
const ARG_DELEGATOR = "delegator";
const ARG_VALIDATOR = "validator";
const ARG_NEW_VALIDATOR = "new_validator";
const METHOD_UNDELEGATE = "undelegate";

export function call(): void {
    let auction = CL.getSystemContract(CL.SystemContract.Auction);

    let delegatorBytes = CL.getNamedArg(ARG_DELEGATOR);
    if (delegatorBytes === null) {
        Error.fromErrorCode(ErrorCode.MissingArgument).revert();
        return;
    }

    let delegatorResult = PublicKey.fromBytes(delegatorBytes);
    if (delegatorResult.hasError()) {
        Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
        return;
    }
    let delegator = delegatorResult.value;

    let validatorBytes = CL.getNamedArg(ARG_VALIDATOR);
    if (validatorBytes === null) {
        Error.fromErrorCode(ErrorCode.MissingArgument).revert();
        return;
    }

    let validatorResult = PublicKey.fromBytes(validatorBytes);
    if (validatorResult.hasError()) {
        Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
        return;
    }
    let validator = validatorResult.value;

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

    let newValidatorPublicKeyBytes = CL.getNamedArg(ARG_NEW_VALIDATOR);
    let newValidatorPublicKey = Option.fromBytes(newValidatorPublicKeyBytes);

    let runtimeArgs = RuntimeArgs.fromArray([
        new Pair(ARG_AMOUNT, CLValue.fromU512(amount)),
        new Pair(ARG_DELEGATOR, CLValue.fromPublicKey(delegator)),
        new Pair(ARG_VALIDATOR, CLValue.fromPublicKey(validator)),
        new Pair(ARG_NEW_VALIDATOR, CLValue.fromOption(newValidatorPublicKey, new CLType(CLTypeTag.PublicKey)))
    ]);
    CL.callContract(auction, METHOD_UNDELEGATE, runtimeArgs);
}
