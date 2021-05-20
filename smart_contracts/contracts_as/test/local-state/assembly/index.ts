// The entry file of your WebAssembly module.
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {removeAssociatedKey, RemoveKeyFailure} from "../../../../contract_as/assembly/account";
import {typedToArray} from "../../../../contract_as/assembly/utils";
import {AccountHash} from "../../../../contract_as/assembly/key";
import {CLValue, CLType, CLTypeTag} from "../../../../contract_as/assembly/clvalue";
import { Key } from "../../../../contract_as/assembly/key";
import { AccessRights, URef } from "../../../../contract_as/assembly/uref";
import { Pair } from "../../../../contract_as/assembly/pair";
import { createLocal, readLocal, writeLocal } from "../../../../contract_as/assembly/local";
import { fromBytesString, toBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { revert } from "../../../../contract_as/assembly/externals";

const LOCAL_KEY_NAME = "local";
const WRITE_LOCAL_KEY: Array<u8> = [66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66];
const HELLO_PREFIX = " Hello, ";
const WORLD_SUFFIX = "world!";
const MODIFY_WRITE_ENTRYPOINT = "modify_write";
const SHARE_RO_ENTRYPOINT = "share_ro";
const SHARE_W_ENTRYPOINT = "share_w";
const CONTRACT_HASH_NAME = "contract_hash";
const CONTRACT_PACKAGE_HASH_NAME = "package_hash_name";
const DEFAULT_LOCAL_KEY_NAME = "Default Key";
const DEFAULT_LOCAL_KEY_VALUE = "Default Value";
const ACCESS_KEY_NAME = "access_key";

function getLocalURef(): URef {
  let key = CL.getKey(LOCAL_KEY_NAME);
  if (key === null) {
    Error.fromUserError(0).revert();
    return <URef>unreachable();
  }
  const localURef = key.uref;
  if (localURef === null) {
    Error.fromUserError(1).revert();
    return <URef>unreachable();
  }
  return localURef;
}

export function modify_write(): void {
  let localURef = getLocalURef();

  let res1: String;

  let resBytes1 = readLocal(localURef, WRITE_LOCAL_KEY);
  if (resBytes1 !== null) {
    res1 = fromBytesString(resBytes1).unwrap();
  }
  else {
    res1 = "";
  }
  
  res1 += HELLO_PREFIX;
  writeLocal(localURef, WRITE_LOCAL_KEY, CLValue.fromString(res1));

  // Read (this should exercise cache)
  const resBytes2 = readLocal(localURef, WRITE_LOCAL_KEY);
  if (resBytes2 === null) {
    Error.fromUserError(3).revert();
    return;
  }
  let res2 = fromBytesString(resBytes2).unwrap();
  res2 += WORLD_SUFFIX;
  writeLocal(localURef, WRITE_LOCAL_KEY, CLValue.fromString(res2.trim()));
}

export function share_ro(): void {
  let localURef = getLocalURef();
  localURef.setAccessRights(AccessRights.READ);

  CL.ret(CLValue.fromURef(localURef));
}

export function share_w(): void {
  let localURef = getLocalURef();
  localURef.setAccessRights(AccessRights.WRITE);

  CL.ret(CLValue.fromURef(localURef));
}


export function call(): void {
  let entryPoints = new CL.EntryPoints();

  let entryPointModifyWrite = new CL.EntryPoint("modify_write", new Array(), new CLType(CLTypeTag.Unit), new CL.PublicAccess(), CL.EntryPointType.Contract);
  entryPoints.addEntryPoint(entryPointModifyWrite);
  let entryPointShareRo = new CL.EntryPoint("share_ro", new Array(), new CLType(CLTypeTag.Uref), new CL.PublicAccess(), CL.EntryPointType.Contract);
  entryPoints.addEntryPoint(entryPointShareRo);
  let entryPointShareW = new CL.EntryPoint("share_w", new Array(), new CLType(CLTypeTag.Uref), new CL.PublicAccess(), CL.EntryPointType.Contract);
  entryPoints.addEntryPoint(entryPointShareW);

  let namedKeys = new Array<Pair<String, Key>>();

  let localURef = createLocal();
  writeLocal(localURef, toBytesString(DEFAULT_LOCAL_KEY_NAME), CLValue.fromString(DEFAULT_LOCAL_KEY_VALUE));

  namedKeys.push(new Pair(LOCAL_KEY_NAME, Key.fromURef(localURef)))

  const result = CL.newContract(
    entryPoints,
    namedKeys,
    CONTRACT_PACKAGE_HASH_NAME,
    ACCESS_KEY_NAME,
  );
  const key = Key.create(CLValue.fromI32(result.contractVersion));
  if (key === null) {
    return;
  }
  CL.putKey(CONTRACT_HASH_NAME, Key.fromHash(result.contractHash));
}
