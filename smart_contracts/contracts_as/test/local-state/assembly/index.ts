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
import { newDictionary, dictionaryGet, dictionaryPut } from "../../../../contract_as/assembly/local";
import { fromBytesString, toBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { revert } from "../../../../contract_as/assembly/externals";

const DICTIONARY_NAME = "local";
const DICTIONARY_PUT: Array<u8> = [66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66];
const HELLO_PREFIX = " Hello, ";
const WORLD_SUFFIX = "world!";
const MODIFY_WRITE_ENTRYPOINT = "modify_write";
const SHARE_RO_ENTRYPOINT = "share_ro";
const SHARE_W_ENTRYPOINT = "share_w";
const CONTRACT_HASH_NAME = "contract_hash";
const CONTRACT_PACKAGE_HASH_NAME = "package_hash_name";
const DEFAULT_DICTIONARY_NAME = "Default Key";
const DEFAULT_DICTIONARY_VALUE = "Default Value";
const ACCESS_KEY_NAME = "access_key";
const DICTIONARY_REF = "new_dictionary";

function getDictionaryURef(): URef {
  let key = CL.getKey(DICTIONARY_NAME);
  if (key === null) {
    Error.fromUserError(0).revert();
    return <URef>unreachable();
  }
  const dictionaryURef = key.uref;
  if (dictionaryURef === null) {
    Error.fromUserError(1).revert();
    return <URef>unreachable();
  }
  return dictionaryURef;
}

export function modify_write(): void {
  let dictionaryURef = getDictionaryURef();

  let res1: String;

  let resBytes1 = dictionaryGet(dictionaryURef, DICTIONARY_PUT);
  if (resBytes1 !== null) {
    res1 = fromBytesString(resBytes1).unwrap();
  }
  else {
    res1 = "";
  }
  
  res1 += HELLO_PREFIX;
  dictionaryPut(dictionaryURef, DICTIONARY_PUT, CLValue.fromString(res1));

  // Read (this should exercise cache)
  const resBytes2 = dictionaryGet(dictionaryURef, DICTIONARY_PUT);
  if (resBytes2 === null) {
    Error.fromUserError(3).revert();
    return;
  }
  let res2 = fromBytesString(resBytes2).unwrap();
  res2 += WORLD_SUFFIX;
  dictionaryPut(dictionaryURef, DICTIONARY_PUT, CLValue.fromString(res2.trim()));
}

export function share_ro(): void {
  let dictionaryURef = getDictionaryURef();
  dictionaryURef.setAccessRights(AccessRights.READ);

  CL.ret(CLValue.fromURef(dictionaryURef));
}

export function share_w(): void {
  let dictionaryURef = getDictionaryURef();
  dictionaryURef.setAccessRights(AccessRights.WRITE);

  CL.ret(CLValue.fromURef(dictionaryURef));
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

  let dictionaryURef = newDictionary(DICTIONARY_REF);
  dictionaryPut(dictionaryURef, toBytesString(DEFAULT_DICTIONARY_NAME), CLValue.fromString(DEFAULT_DICTIONARY_VALUE));

  namedKeys.push(new Pair(DICTIONARY_NAME, Key.fromURef(dictionaryURef)))

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
