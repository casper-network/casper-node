//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { Pair } from "../../../../contract_as/assembly/pair";
import { URef } from "../../../../contract_as/assembly/uref";
import { Pair } from "../../../../contract_as/assembly/pair";
import { createPurse } from "../../../../contract_as/assembly/purse";
import { CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import * as CreatePurse01 from "../../create-purse-01/assembly";

const ENTRY_FUNCTION_NAME = "delegate";
const DO_NOTHING_PACKAGE_HASH_KEY_NAME = "do_nothing_package_hash";
const DO_NOTHING_ACCESS_KEY_NAME = "do_nothing_access";

export function delegate(): void {
  let key = new StaticArray<u8>(32);
  for (var i = 0; i < 32; i++) {
    key[i] = 1;
  }
  CL.putKey("called_do_nothing_ver_2", Key.fromHash(key));
  CreatePurse01.delegate();
}

export function call(): void {
  let entryPoints = new CL.EntryPoints();

  let entryPointArgs = new Array<Pair<String, CLType>>();
  entryPointArgs.push(new Pair<String, CLType>("purse_name", new CLType(CLTypeTag.String)));
  let entryPoint = new CL.EntryPoint(
    ENTRY_FUNCTION_NAME,
    entryPointArgs,
    new CLType(CLTypeTag.Unit),
    new CL.PublicAccess(),
    CL.EntryPointType.Session);
  entryPoints.addEntryPoint(entryPoint);

  let doNothingPackageHash = CL.getKey(DO_NOTHING_PACKAGE_HASH_KEY_NAME);
  if (!doNothingPackageHash) {
    Error.fromErrorCode(ErrorCode.None).revert();
    return;
  }

  let doNothingURef = CL.getKey(DO_NOTHING_ACCESS_KEY_NAME);
  if (!doNothingURef) {
    Error.fromErrorCode(ErrorCode.None).revert();
    return;
  }

  const result = CL.addContractVersion(
    <StaticArray<u8>>doNothingPackageHash.hash,
    entryPoints,
    new Array<Pair<String, Key>>(),
  );

  CL.putKey("end of upgrade", Key.fromHash(result.contractHash));
}
