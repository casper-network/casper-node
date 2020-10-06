import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {CLValue} from "../../../../contract_as/assembly/clvalue";
import {RuntimeArgs} from "../../../../contract_as/assembly/runtime_args";
import {Pair} from "../../../../contract_as/assembly/pair";
import {Option} from "../../../../contract_as/assembly/option";
import {URef} from "../../../../contract_as/assembly/uref";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {PublicKey} from "../../../../contract_as/assembly/public_key";

const ARG_AMOUNT = "amount";
const ARG_UNBOND_PURSE = "unbond_purse";
const ARG_DELEGATOR = "delegator";
const ARG_VALIDATOR = "validator";
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

    let maybeUnbondPurseBytes = CL.getNamedArg(ARG_UNBOND_PURSE);
    if (maybeUnbondPurseBytes === null) {
        Error.fromErrorCode(ErrorCode.MissingArgument).revert();
        return;
    }
    let maybeUnbondPurse = Option.fromBytes(maybeUnbondPurseBytes);

    let unbondPurse: URef;
    if (maybeUnbondPurse.isNone()) {
        unbondPurse = getMainPurse();
    }
    else {
        let unbondPurseResult = URef.fromBytes(<Uint8Array>maybeUnbondPurse.unwrap());
        if (unbondPurseResult.hasError()) {
            Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
            return;
        }
        unbondPurse = unbondPurseResult.value;
    }

    let runtimeArgs = RuntimeArgs.fromArray([
        new Pair(ARG_AMOUNT, CLValue.fromU512(amount)),
        new Pair(ARG_DELEGATOR, CLValue.fromPublicKey(delegator)),
        new Pair(ARG_VALIDATOR, CLValue.fromPublicKey(validator)),
        new Pair(ARG_AMOUNT, CLValue.fromU512(amount)),
        new Pair(ARG_UNBOND_PURSE, CLValue.fromURef(unbondPurse)),
    ]);
    CL.callContract(auction, METHOD_UNDELEGATE, runtimeArgs);
}
