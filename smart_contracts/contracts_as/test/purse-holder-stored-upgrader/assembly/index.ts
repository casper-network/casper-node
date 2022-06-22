//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { URef } from "../../../../contract_as/assembly/uref";
import { createPurse } from "../../../../contract_as/assembly/purse";
import { checkItemsEqual } from "../../../../contract_as/assembly/utils";
import { Pair } from "../../../../contract_as/assembly/pair";

const METHOD_ADD = "add";
const METHOD_REMOVE = "remove";
const METHOD_VERSION = "version";
const ARG_PURSE_NAME = "purse_name";
const NEW_VERSION = "1.0.1";
const VERSION = "version";
const ACCESS_KEY_NAME = "purse_holder_access";
const PURSE_HOLDER_STORED_CONTRACT_NAME = "purse_holder_stored";
const ARG_CONTRACT_PACKAGE = "contract_package";

enum CustomError {
  MissingPurseHolderURefArg = 0,
  InvalidPurseHolderURefArg = 1,
  MissingMethodNameArg = 2,
  InvalidMethodNameArg = 3,
  MissingPurseNameArg = 4,
  InvalidPurseNameArg = 5,
  UnknownMethodName = 6,
  UnableToStoreVersion = 7,
  NamedPurseNotCreated = 8
}


function getPurseName(): String {
  let purseNameBytes = CL.getNamedArg(ARG_PURSE_NAME);
  return fromBytesString(purseNameBytes).unwrap();
}

export function add(): void {
  let purseName = getPurseName();
  let purse = createPurse();
  CL.putKey(purseName, Key.fromURef(purse));
}

export function remove(): void {
  let purseName = getPurseName();
  CL.removeKey(purseName);
}

export function version(): void {
  CL.ret(CLValue.fromString(VERSION));
}

export function delegate(): void {
}

export function call(): void {
  let contractPackageHash = CL.getNamedArg(ARG_CONTRACT_PACKAGE);
  let accessKey = CL.getKey(ACCESS_KEY_NAME);
  if (!accessKey) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }

  let entryPoints = new CL.EntryPoints();

  let addArgs = new Array<Pair<String, CLType>>();
  addArgs.push(new Pair(ARG_PURSE_NAME, new CLType(CLTypeTag.String)));

  let add = new CL.EntryPoint(
    METHOD_ADD,
    addArgs,
    new CLType(CLTypeTag.Unit),
    new CL.PublicAccess(),
    CL.EntryPointType.Contract,
  );
  entryPoints.addEntryPoint(add);

  let version = new CL.EntryPoint(
    METHOD_VERSION,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.String),
    new CL.PublicAccess(),
    CL.EntryPointType.Contract,
  );
  entryPoints.addEntryPoint(version);

  let removeArgs = new Array<Pair<String, CLType>>();
  removeArgs.push(new Pair(ARG_PURSE_NAME, new CLType(CLTypeTag.String)));

  let remove = new CL.EntryPoint(
    METHOD_REMOVE,
    removeArgs,
    new CLType(CLTypeTag.Unit),
    new CL.PublicAccess(),
    CL.EntryPointType.Contract,
  );
  entryPoints.addEntryPoint(remove);

  let newResult = CL.addContractVersion(
    contractPackageHash,
    entryPoints,
    new Array<Pair<String, Key>>(),
  );
  CL.putKey(PURSE_HOLDER_STORED_CONTRACT_NAME, Key.fromHash(newResult.contractHash));

  let newVersionKey = Key.create(CLValue.fromString(NEW_VERSION));
  if (!newVersionKey) {
    Error.fromErrorCode(ErrorCode.Formatting).revert();
    return;
  }
  CL.putKey(VERSION, newVersionKey);
}
