//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesString, toBytesMap } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { Pair } from "../../../../contract_as/assembly/pair";

const ENTRY_FUNCTION_NAME = "delegate";
const HASH_KEY_NAME = "do_nothing_hash";
const PACKAGE_HASH_KEY_NAME = "do_nothing_package_hash";
const ACCESS_KEY_NAME = "do_nothing_access";
const CONTRACT_VERSION = "contract_version";

export function delegate(): void {
  // no-op
}

export function call(): void {
  let entryPoints = new CL.EntryPoints();

  let entryPointArgs = new Array<Pair<String, CLType>>();
  entryPointArgs.push(new Pair<String, CLType>("purse_name", new CLType(CLTypeTag.String)));

  let entryPoint = new CL.EntryPoint("delegate", entryPointArgs, new CLType(CLTypeTag.Unit), new CL.PublicAccess(), CL.EntryPointType.Contract);
  entryPoints.addEntryPoint(entryPoint);

  const result = CL.newContract(
    entryPoints,
    null,
    PACKAGE_HASH_KEY_NAME,
    ACCESS_KEY_NAME,
  );
  const key = Key.create(CLValue.fromI32(result.contractVersion));
  if (!key) {
    return;
  }
  CL.putKey(CONTRACT_VERSION, <Key>key);
  CL.putKey(HASH_KEY_NAME, Key.fromHash(result.contractHash));
}
