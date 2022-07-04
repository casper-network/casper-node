//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesString, toBytesMap } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { Pair } from "../../../../contract_as/assembly/pair";
import { putKey, ret } from "../../../../contract_as/assembly";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { createPurse } from "../../../../contract_as/assembly/purse";
import { URef } from "../../../../contract_as/assembly/uref";
import { CLTypeTag } from "../../../../contract_as/assembly/clvalue";

const METHOD_ADD = "add";
const METHOD_REMOVE = "remove";
const METHOD_VERSION = "version";

const ENTRY_POINT_ADD = "add_named_purse";
const ENTRY_POINT_VERSION = "version";
const HASH_KEY_NAME = "purse_holder";
const ACCESS_KEY_NAME = "purse_holder_access";
const ARG_PURSE = "purse_name";
const ARG_IS_LOCKED = "is_locked";
const VERSION = "1.0.0";
const PURSE_HOLDER_STORED_CONTRACT_NAME = "purse_holder_stored";

enum CustomError {
  MissingMethodNameArg = 0,
  InvalidMethodNameArg = 1,
  MissingPurseNameArg = 2,
  InvalidPurseNameArg = 3,
  UnknownMethodName = 4,
  NamedPurseNotCreated = 5
}

export function add_named_purse(): void {
  const purseNameBytes = CL.getNamedArg(ARG_PURSE);
  const purseName = fromBytesString(purseNameBytes).unwrap();
  const purse = createPurse();
  CL.putKey(purseName, Key.fromURef(purse));
}

export function version(): void {
  CL.ret(CLValue.fromString(VERSION));
}

export function call(): void {
  let entryPoints = new CL.EntryPoints();
  let is_locked = CL.getNamedArg(ARG_IS_LOCKED);

  {
    let args = new Array<Pair<String, CLType>>();
    args.push(new Pair(ARG_PURSE, new CLType(CLTypeTag.String)));
    let entryPointAdd = new CL.EntryPoint(ENTRY_POINT_ADD, args, new CLType(CLTypeTag.Unit), new CL.PublicAccess(), CL.EntryPointType.Contract);
    entryPoints.addEntryPoint(entryPointAdd);
  }
  {
    let entryPointAdd = new CL.EntryPoint(ENTRY_POINT_VERSION, new Array<Pair<String, CLType>>(), new CLType(CLTypeTag.Unit), new CL.PublicAccess(), CL.EntryPointType.Contract);
    entryPoints.addEntryPoint(entryPointAdd);
  }

  if (is_locked[0] == true) {
    let result = CL.newLockedContract(
      entryPoints,
      null,
      HASH_KEY_NAME,
      ACCESS_KEY_NAME
    );
    putKey(PURSE_HOLDER_STORED_CONTRACT_NAME, Key.fromHash(result.contractHash));
    const versionKey = Key.create(CLValue.fromString(VERSION));
    if (!versionKey) {
      Error.fromErrorCode(ErrorCode.Formatting).revert();
    }
    putKey(ENTRY_POINT_VERSION, <Key>versionKey);

  } else if (is_locked[0] == false) {
    let result = CL.newContract(
      entryPoints,
      null,
      HASH_KEY_NAME,
      ACCESS_KEY_NAME);

    putKey(PURSE_HOLDER_STORED_CONTRACT_NAME, Key.fromHash(result.contractHash));
    const versionKey = Key.create(CLValue.fromString(VERSION));
    if (!versionKey) {
      Error.fromErrorCode(ErrorCode.Formatting).revert();
    }
    putKey(ENTRY_POINT_VERSION, <Key>versionKey);
  }
}
