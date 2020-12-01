//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import {Error} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {Key} from "../../../../contract_as/assembly/key";
import {URef} from "../../../../contract_as/assembly/uref";
import {putKey} from "../../../../contract_as/assembly";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {fromBytesString} from "../../../../contract_as/assembly/bytesrepr";
import {createPurse, transferFromPurseToPurse} from "../../../../contract_as/assembly/purse";


const ARG_AMOUNT = "amount";
const ARG_DESTINATION = "destination";

enum CustomError{
    InvalidAmountArg = 2,
    InvalidDestinationArg = 4
}

export function call(): void {
    const amountArg = CL.getNamedArg(ARG_AMOUNT);
    const amountResult = U512.fromBytes(amountArg);
    if (amountResult.hasError()) {
        Error.fromUserError(<u16>CustomError.InvalidAmountArg).revert();
        return;
    }
    let amount = amountResult.value;
    const destinationPurseNameArg = CL.getNamedArg(ARG_DESTINATION);
    const destinationPurseNameResult = fromBytesString(destinationPurseNameArg);
    if (destinationPurseNameResult.hasError()) {
        Error.fromUserError(<u16>CustomError.InvalidDestinationArg).revert();
        return;
    }
    let destinationPurseName = destinationPurseNameResult.value;
    const mainPurse = getMainPurse();
    const destinationPurse = createPurse();
    const error = transferFromPurseToPurse(mainPurse, destinationPurse, <U512>amount);
    if (error !== null) {
        error.revert();
        return;
    }
    putKey(destinationPurseName, <Key>Key.fromURef(destinationPurse));
}
