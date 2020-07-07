//@ts-nocheck
import * as CL from "../../../../contract-as/assembly";
import {Error} from "../../../../contract-as/assembly/error";
import {fromBytesString} from "../../../../contract-as/assembly/bytesrepr";
import {Key} from "../../../../contract-as/assembly/key";
import {putKey} from "../../../../contract-as/assembly";
import {CLValue} from "../../../../contract-as/assembly/clvalue";
import { RuntimeArgs } from "../../../../contract-as/assembly/runtime_args";
import {Pair} from "../../../../contract-as/assembly/pair";

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
    if (versionBytes === null) {
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
    if (maybeVersionKey === null) {
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
