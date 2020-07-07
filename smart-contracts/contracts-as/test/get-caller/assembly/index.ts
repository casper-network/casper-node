import * as CL from "../../../../contract-as/assembly";
import {Error, ErrorCode} from "../../../../contract-as/assembly/error";
import {typedToArray, checkArraysEqual} from "../../../../contract-as/assembly/utils";
import {AccountHash} from "../../../../contract-as/assembly/key";

const ARG_ACCOUNT = "account";

export function call(): void {
  const knownAccountHashBytes = CL.getNamedArg(ARG_ACCOUNT);
  let knownAccountHashResult = AccountHash.fromBytes(knownAccountHashBytes);
  if (knownAccountHashResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }
  const knownAccountHash = knownAccountHashResult.value;
  const caller = CL.getCaller();

  assert(caller == knownAccountHash);
}
