import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {typedToArray, checkArraysEqual} from "../../../../contract_as/assembly/utils";
import {AccountHash} from "../../../../contract_as/assembly/key";

const ARG_ACCOUNT = "account";

export function call(): void {
  const knownAccountHashBytes = CL.getNamedArg(ARG_ACCOUNT);
  let knownAccountHashResult = AccountHash.fromBytes(knownAccountHashBytes.slice(0));
  if (knownAccountHashResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }
  const knownAccountHash = knownAccountHashResult.value;
  const caller = CL.getCaller();

  assert(caller == knownAccountHash);
}
