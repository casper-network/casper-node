import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {CLValue} from "../../../../contract_as/assembly/clvalue";
import {RuntimeArgs} from "../../../../contract_as/assembly/runtime_args";
import {Pair} from "../../../../contract_as/assembly/pair";
import {PublicKey} from "../../../../contract_as/assembly/public_key";

const ARG_AMOUNT = "amount";
const ARG_PUBLIC_KEY = "public_key";
const ARG_UNBOND_PURSE = "unbond_purse";
const METHOD_WITHDRAW_BID = "withdraw_bid";

export function call(): void {
    let auction = CL.getSystemContract(CL.SystemContract.Auction);

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
    
    let runtimeArgs = RuntimeArgs.fromArray([
        new Pair(ARG_AMOUNT, CLValue.fromU512(amount)),
        new Pair(ARG_PUBLIC_KEY, CLValue.fromPublicKey(publicKey)),
    ]);

    CL.callContract(auction, METHOD_WITHDRAW_BID, runtimeArgs);
}
