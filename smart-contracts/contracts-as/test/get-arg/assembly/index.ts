//@ts-nocheck
import * as CL from "../../../../contract-as/assembly";
import {Error, ErrorCode} from "../../../../contract-as/assembly/error";
import {U512} from "../../../../contract-as/assembly/bignum";
import {fromBytesString} from "../../../../contract-as/assembly/bytesrepr";

const EXPECTED_STRING =  "Hello, world!";
const EXPECTED_NUM = 42;

const ARG_VALUE0 = "value0";
const ARG_VALUE1 = "value1";

export function call(): void {
  const stringArg = CL.getNamedArg(ARG_VALUE0);
  const stringValResult = fromBytesString(stringArg)
  if (stringValResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }
  let stringVal = stringValResult.value;
  if (stringVal != EXPECTED_STRING){
    unreachable();
    return;
  }
  const u512Arg = CL.getNamedArg(ARG_VALUE1);
  const u512ValResult = U512.fromBytes(u512Arg);
  if (u512ValResult.hasError() || <usize>u512Arg.length > u512ValResult.position) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;    
  }
  let u512Val = u512ValResult.value;
  if (u512Val != U512.fromU64(EXPECTED_NUM)){
    unreachable();
    return;
  }
}
