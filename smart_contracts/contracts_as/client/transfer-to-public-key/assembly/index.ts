import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {transferToPublicKey} from "../../../../contract_as/assembly/purse";
import {PublicKey} from "../../../../contract_as/assembly/public_key";

const ARG_TARGET = "target";
const ARG_AMOUNT = "amount";

export function call(): void {
    const publicKeyBytes = CL.getNamedArg(ARG_TARGET);
    const publicKey = PublicKey.fromBytes(publicKeyBytes).unwrap();

    const amountBytes = CL.getNamedArg(ARG_AMOUNT);
    const amount = U512.fromBytes(amountBytes).unwrap();

    const result = transferToPublicKey(publicKey, amount);
    if (result.isErr) {
        result.err.revert();
        return;
    }
}
