//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error } from "../../../../contract_as/assembly/error";
import { fromBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { putKey } from "../../../../contract_as/assembly";
import { CLValue } from "../../../../contract_as/assembly/clvalue";
import { RuntimeArgs } from "../../../../contract_as/assembly/runtime_args";
import { Pair } from "../../../../contract_as/assembly/pair";

const METHOD_VERSION = "version";
const HASH_KEY_NAME = "purse_holder";
const ENTRY_POINT_NAME = "entry_point";
const PURSE_NAME = "purse_name";

enum CustomError {
  UnableToGetVersion = 6,
  UnableToStoreVersion = 7,
  InvalidVersion = 8
}

export function call(): void {
  let entryPointNameBytes = CL.getNamedArg(ENTRY_POINT_NAME);
  let entryPointName = fromBytesString(entryPointNameBytes).unwrap();

  // short circuit if VERSION method called
  if (entryPointName == METHOD_VERSION) {
    let contractHash = CL.getNamedArg(HASH_KEY_NAME);
    const versionBytes = CL.callContract(contractHash, entryPointName, new RuntimeArgs());
    if (!versionBytes) {
      Error.fromUserError(<u16>CustomError.UnableToGetVersion).revert();
      return;
    }
    const versionResult = fromBytesString(versionBytes);
    if (versionResult.hasError()) {
      Error.fromUserError(<u16>CustomError.InvalidVersion).revert();
      return;
    }
    let version = versionResult.value;
    const maybeVersionKey = Key.create(CLValue.fromString(version));
    if (!maybeVersionKey) {
      Error.fromUserError(<u16>CustomError.UnableToStoreVersion).revert();
      return;
    }
    const versionKey = <Key>maybeVersionKey;
    putKey(METHOD_VERSION, versionKey);
  }
  else {
    let contractHash = CL.getNamedArg(HASH_KEY_NAME);
    let purseNameBytes = CL.getNamedArg(PURSE_NAME);
    let purseName = fromBytesString(purseNameBytes).unwrap();
    let runtimeArgs = RuntimeArgs.fromArray([
      new Pair(PURSE_NAME, CLValue.fromString(purseName)),
    ]);
    CL.callContract(contractHash, entryPointName, runtimeArgs);
  }
}
