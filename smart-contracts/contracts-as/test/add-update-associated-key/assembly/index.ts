// The entry file of your WebAssembly module.
import * as CL from "../../../../contract-as/assembly";
import {Error, ErrorCode} from "../../../../contract-as/assembly/error";
import {addAssociatedKey, AddKeyFailure, updateAssociatedKey, UpdateKeyFailure} from "../../../../contract-as/assembly/account";
import {typedToArray} from "../../../../contract-as/assembly/utils";
import {AccountHash} from "../../../../contract-as/assembly/key";


const INIT_WEIGHT: u8 = 1;
const MOD_WEIGHT: u8 = 2;

const ARG_ACCOUNT = "account";

export function call(): void {
  let accountHashBytes = CL.getNamedArg(ARG_ACCOUNT);
  const accountHashResult = AccountHash.fromBytes(accountHashBytes);
  if (accountHashResult.hasError()) {
    Error.fromUserError(<u16>4464 + <u16>accountHashResult.error).revert();
    return;
  }
  const accountHash = accountHashResult.value;
  
  if (addAssociatedKey(accountHash, INIT_WEIGHT) != AddKeyFailure.Ok) {
    Error.fromUserError(<u16>4464).revert();
    return;
  }

  if (updateAssociatedKey(accountHash, MOD_WEIGHT) != UpdateKeyFailure.Ok) {
    Error.fromUserError(<u16>4464 + 1).revert();
    return;
  }
}
