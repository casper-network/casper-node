// The entry file of your WebAssembly module.
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {removeAssociatedKey, RemoveKeyFailure} from "../../../../contract_as/assembly/account";
import {typedToArray} from "../../../../contract_as/assembly/utils";
import {AccountHash} from "../../../../contract_as/assembly/key";

const ARG_ACCOUNT = "account";

export function call(): void {
  let accountBytes = CL.getNamedArg(ARG_ACCOUNT);
  const accountResult = AccountHash.fromBytes(accountBytes.slice(0));
  if (accountResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }
  const account = accountResult.value;

  if (removeAssociatedKey(account) != RemoveKeyFailure.Ok) {
    Error.fromUserError(<u16>4464).revert();
    return;
  }
}
