//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { Key } from "../../../../contract_as/assembly/key";
import { URef } from "../../../../contract_as/assembly/uref";
import { fromBytesString, fromBytesU32, fromBytesU64, Result, fromBytesArray } from "../../../../contract_as/assembly/bytesrepr";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { Pair } from "../../../../contract_as/assembly/pair";
import { RuntimeArgs } from "../../../../contract_as/assembly/runtime_args";
import { get_named_arg } from "../../../../contract_as/assembly/externals";

const PACKAGE_HASH_KEY = "package_hash_key";
const PACKAGE_ACCESS_KEY = "package_access_key";
const CREATE_GROUP = "create_group";
const REMOVE_GROUP = "remove_group";
const EXTEND_GROUP_UREFS = "extend_group_urefs";
const REMOVE_GROUP_UREFS = "remove_group_urefs";
const GROUP_NAME_ARG = "group_name";
const UREFS_ARG = "urefs";
const TOTAL_NEW_UREFS_ARG = "total_new_urefs";
const TOTAL_EXISTING_UREFS_ARG = "total_existing_urefs";

export function create_group(): void {
  let packageHashKey = CL.getKey(PACKAGE_HASH_KEY);
  if (packageHashKey === null) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }
  let packageAccessKey = CL.getKey(PACKAGE_ACCESS_KEY);
  if (packageAccessKey === null) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }
  let group_name: String = fromBytesString(CL.getNamedArg(GROUP_NAME_ARG)).unwrap();
  let total_urefs: u64 = fromBytesU64(CL.getNamedArg(TOTAL_NEW_UREFS_ARG)).unwrap();
  let total_existing_urefs: u64 = fromBytesU64(CL.getNamedArg(TOTAL_EXISTING_UREFS_ARG)).unwrap();

  let existingURefs = new Array<URef>();
  for (var i: u64 = 0; i < total_existing_urefs; i++) {
    let res = Key.create(CLValue.fromU64(i));
    if (res === null) {
      Error.fromErrorCode(ErrorCode.Formatting).revert();
      unreachable();
      return;
    }
    existingURefs.push(<URef>res.uref);
  }

  let newURefs = CL.createContractUserGroup(
    <Uint8Array>packageHashKey.hash,
    group_name,
    total_urefs as u8,
    existingURefs,
  );
}

export function remove_group(): void {
  let packageHashKey = CL.getKey(PACKAGE_HASH_KEY);
  if (packageHashKey === null) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }
  let groupName: String = fromBytesString(CL.getNamedArg(GROUP_NAME_ARG)).unwrap();
  CL.removeContractUserGroup(
    <Uint8Array>packageHashKey.hash,
    groupName);
}

export function extend_group_urefs(): void {
  let packageHashKey = CL.getKey(PACKAGE_HASH_KEY);
  if (packageHashKey === null) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }
  let packageAccessKey = CL.getKey(PACKAGE_ACCESS_KEY);
  if (packageAccessKey === null) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }
  let groupName: String = fromBytesString(CL.getNamedArg(GROUP_NAME_ARG)).unwrap();
  let newURefsCount: u64 = fromBytesU64(CL.getNamedArg(TOTAL_NEW_UREFS_ARG)).unwrap();

  // Creates 1 additional uref inside group
  for (var i = <u64>0; i < newURefsCount; i++) {
    let _newURef = CL.extendContractUserGroupURefs(
      <Uint8Array>packageHashKey.hash,
      groupName
    );
  }
}

export function remove_group_urefs(): void {
  let packageHashKey = CL.getKey(PACKAGE_HASH_KEY);
  if (packageHashKey === null) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }
  let groupName: String = fromBytesString(CL.getNamedArg(GROUP_NAME_ARG)).unwrap();

  let ordinals = fromBytesArray(CL.getNamedArg(GROUP_NAME_ARG), fromBytesU64);

  let contractPackageBytes = packageHashKey.read();
  if (contractPackageBytes === null) {
    Error.fromErrorCode(ErrorCode.GetKey).revert();
    return;
  }

  // Finds last uref in the groups in the contract package bytes.
  // ContractPackage serialization structure is defined in `types/src/contracts.rs`
  let urefBytes = contractPackageBytes.slice(contractPackageBytes.length - 1 - 33, contractPackageBytes.length - 1);
  let uref = URef.fromBytes(urefBytes).unwrap();

  let urefs = new Array<URef>();
  urefs.push(uref);

  CL.removeContractUserGroupURefs(
    <Uint8Array>packageHashKey.hash,
    groupName,
    urefs,
  );
}

/// Restricted uref comes from creating a group and will be assigned to a smart contract
function createEntryPoints1(): CL.EntryPoints {
  let entryPoints = new CL.EntryPoints();

  {
    let restrictedSessionParams = new Array<Pair<String, CLType>>();
    restrictedSessionParams.push(new Pair(GROUP_NAME_ARG, new CLType(CLTypeTag.String)));
    restrictedSessionParams.push(new Pair(TOTAL_EXISTING_UREFS_ARG, new CLType(CLTypeTag.U64)));
    restrictedSessionParams.push(new Pair(TOTAL_NEW_UREFS_ARG, new CLType(CLTypeTag.U64)));
    let restrictedSession = new CL.EntryPoint(
      CREATE_GROUP,
      restrictedSessionParams,
      new CLType(CLTypeTag.Unit),
      new CL.PublicAccess(),
      CL.EntryPointType.Session,
    );
    entryPoints.addEntryPoint(restrictedSession);
  }

  {
    let params = new Array<Pair<String, CLType>>();
    params.push(new Pair(GROUP_NAME_ARG, new CLType(CLTypeTag.String)));

    let removeGroup = new CL.EntryPoint(
      REMOVE_GROUP,
      params,
      new CLType(CLTypeTag.Unit),
      new CL.PublicAccess(),
      CL.EntryPointType.Session,
    );
    entryPoints.addEntryPoint(removeGroup);
  }

  {
    let params = new Array<Pair<String, CLType>>();
    params.push(new Pair(GROUP_NAME_ARG, new CLType(CLTypeTag.String)));
    params.push(new Pair(TOTAL_NEW_UREFS_ARG, new CLType(CLTypeTag.U64)));
    let extendGroupURefs = new CL.EntryPoint(
      EXTEND_GROUP_UREFS,
      params,
      new CLType(CLTypeTag.Unit),
      new CL.PublicAccess(),
      CL.EntryPointType.Session,
    );
    entryPoints.addEntryPoint(extendGroupURefs);
  }

  {
    let params = new Array<Pair<String, CLType>>();
    params.push(new Pair(GROUP_NAME_ARG, new CLType(CLTypeTag.String)));
    params.push(new Pair(UREFS_ARG, CLType.list(new CLType(CLTypeTag.Uref))));

    let entry_point_name2 = REMOVE_GROUP_UREFS;
    let remove_group_urefs = new CL.EntryPoint(
      entry_point_name2,
      params,
      new CLType(CLTypeTag.Unit),
      new CL.PublicAccess(),
      CL.EntryPointType.Session,
    );
    entryPoints.addEntryPoint(remove_group_urefs);
  }
  return entryPoints;
}

function installVersion1(package_hash: Uint8Array): void {
  let contractNamedKeys = new Array<Pair<String, Key>>();
  let entryPoints = createEntryPoints1();
  const result = CL.addContractVersion(package_hash, entryPoints, contractNamedKeys);
}

export function call(): void {
  let result = CL.createContractPackageAtHash();

  CL.putKey(PACKAGE_HASH_KEY, Key.fromHash(result.packageHash));
  CL.putKey(PACKAGE_ACCESS_KEY, Key.fromURef(result.accessURef));

  installVersion1(result.packageHash);
}
