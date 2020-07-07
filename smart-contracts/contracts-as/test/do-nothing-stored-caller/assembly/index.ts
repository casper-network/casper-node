//@ts-nocheck
import * as CL from "../../../../contract-as/assembly";
import {fromBytesString, toBytesU32} from "../../../../contract-as/assembly/bytesrepr";
import {CLValue} from "../../../../contract-as/assembly/clvalue";
import {RuntimeArgs} from "../../../../contract-as/assembly/runtime_args";
import {Pair} from "../../../../contract-as/assembly/pair";
import {Option} from "../../../../contract-as/assembly/option";
import {arrayToTyped} from "../../../../contract-as/assembly/utils";

const ENTRY_FUNCTION_NAME = "delegate";
const PURSE_NAME_ARG_NAME = "purse_name";
const ARG_CONTRACT_PACKAGE = "contract_package";
const ARG_NEW_PURSE_NAME = "new_purse_name";
const ARG_VERSION = "version";

export function call(): void {
  let contractPackageHash = CL.getNamedArg(ARG_CONTRACT_PACKAGE);
  const newPurseNameBytes = CL.getNamedArg(ARG_NEW_PURSE_NAME);
  const newPurseName = fromBytesString(newPurseNameBytes).unwrap();
  const versionNumber = CL.getNamedArg(ARG_VERSION)[0];
  let contractVersion = new Option(arrayToTyped(toBytesU32(versionNumber)));
  let runtimeArgs = RuntimeArgs.fromArray([
    new Pair(PURSE_NAME_ARG_NAME, CLValue.fromString(newPurseName)),
  ]);
  CL.callVersionedContract(contractPackageHash, contractVersion, ENTRY_FUNCTION_NAME, runtimeArgs);
}
