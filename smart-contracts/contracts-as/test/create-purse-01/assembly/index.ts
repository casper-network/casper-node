//@ts-nocheck
import * as CL from "../../../../contract-as/assembly";
import {Error, ErrorCode} from "../../../../contract-as/assembly/error";
import {fromBytesString} from "../../../../contract-as/assembly/bytesrepr";
import {Key} from "../../../../contract-as/assembly/key";
import {putKey} from "../../../../contract-as/assembly";
import {createPurse} from "../../../../contract-as/assembly/purse";
import {URef} from "../../../../contract-as/assembly/uref";

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
  
    const key = Key.fromURef(purse);
    putKey(purseName, <Key>key);
}

export function call(): void {
  delegate();
}
