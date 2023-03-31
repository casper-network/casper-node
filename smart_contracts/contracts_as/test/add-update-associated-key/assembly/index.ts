// The entry file of your WebAssembly module.
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {addAssociatedKey, AddKeyFailure, updateAssociatedKey, UpdateKeyFailure} from "../../../../contract_as/assembly/account";
import {typedToArray} from "../../../../contract_as/assembly/utils";
import {AccountHash} from "../../../../contract_as/assembly/key";


const INIT_WEIGHT: u8 = 1;
const MOD_WEIGHT: u8 = 2;

const ARG_ACCOUNT = "account";

export function call(): void {
  let accountHashBytes = CL.getNamedArg(ARG_ACCOUNT);
  const accountHashResult = AccountHash.fromBytes(accountHashBytes.slice(0));
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
