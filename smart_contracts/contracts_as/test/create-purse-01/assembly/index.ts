//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {fromBytesString} from "../../../../contract_as/assembly/bytesrepr";
import {Key} from "../../../../contract_as/assembly/key";
import {putKey} from "../../../../contract_as/assembly";
import {createPurse} from "../../../../contract_as/assembly/purse";
import {URef} from "../../../../contract_as/assembly/uref";

const ARG_PURSE_NAME = "purse_name";

export function delegate(): void {
    // purse name arg
    const purseNameArg = CL.getNamedArg(ARG_PURSE_NAME);
    const purseNameResult = fromBytesString(purseNameArg);
    if (purseNameResult.hasError()) {
      Error.fromErrorCode(ErrorCode.PurseNotCreated).revert();
      return;
    }
    let purseName = purseNameResult.value;

    const purse = createPurse();

    const purseKey = Key.fromURef(purse);
    putKey(purseName, <Key>purseKey);
}

export function call(): void {
  delegate();
}
