import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {CLValue} from "../../../../contract_as/assembly/clvalue";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {RuntimeArgs} from "../../../../contract_as/assembly/runtime_args";
import {Pair} from "../../../../contract_as/assembly/pair";
import {fromBytesU8} from "../../../../contract_as/assembly/bytesrepr";
import {PublicKey} from "../../../../contract_as/assembly/public_key";

const ARG_PUBLIC_KEY = "public_key";
const ARG_AMOUNT = "amount";
const ARG_DELEGATION_RATE = "delegation_rate";

const METHOD_ADD_BID = "add_bid";

export function call(): void {
    let auction = CL.getAuction();
    let mainPurse = getMainPurse();

    let publicKeyBytes = CL.getNamedArg(ARG_PUBLIC_KEY);
    if (publicKeyBytes === null) {
        Error.fromErrorCode(ErrorCode.MissingArgument).revert();
        return;
    }

    let publicKeyResult = PublicKey.fromBytes(publicKeyBytes);
    if (publicKeyResult.hasError()) {
        Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
        return;
    }
    let publicKey = publicKeyResult.value;

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

    let delegationRateBytes = CL.getNamedArg(ARG_DELEGATION_RATE);
    if (delegationRateBytes === null) {
        Error.fromErrorCode(ErrorCode.MissingArgument).revert();
        return;
    }

    let delegationRateResult = fromBytesU8(delegationRateBytes);
    if (delegationRateResult.hasError()) {
        Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
        return;
    }

    let delegationRate = delegationRateResult.value;

    let runtimeArgs = RuntimeArgs.fromArray([
        new Pair(ARG_PUBLIC_KEY, CLValue.fromPublicKey(publicKey)),
        new Pair(ARG_AMOUNT, CLValue.fromU512(amount)),
        new Pair(ARG_DELEGATION_RATE, CLValue.fromU8(delegationRate)),
    ]);
    CL.callContract(auction, METHOD_ADD_BID, runtimeArgs);
}
