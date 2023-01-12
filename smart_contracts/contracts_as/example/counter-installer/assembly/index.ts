//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesI32, fromBytesString, toBytesMap } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { Pair } from "../../../../contract_as/assembly/pair";

const COUNT_KEY = "count";
const COUNTER_INC = "counter_inc";
const COUNTER_GET = "counter_get";
const COUNTER_KEY = "counter";
const CONTRACT_VERSION_KEY = "version";

export function counter_inc(): void {
  let countKey = CL.getKey(COUNT_KEY);
  if (!countKey) {
    Error.fromErrorCode(ErrorCode.MissingKey).revert();
    return;
  }

  if (!countKey.isURef()) {
    Error.fromErrorCode(ErrorCode.UnexpectedKeyVariant).revert();
    return;
  }

  const one = CLValue.fromI32(1);

  countKey.add(one);
}

export function counter_get(): void {
  let countKey = CL.getKey(COUNT_KEY);

  if (!countKey) {
    Error.fromErrorCode(ErrorCode.MissingKey).revert();
    return;
  }

  if (!countKey.isURef()) {
    Error.fromErrorCode(ErrorCode.UnexpectedKeyVariant).revert();
    return;
  }
  let countData = countKey.read();
  if (!countData) {
    Error.fromErrorCode(ErrorCode.ValueNotFound).revert();
    return;
  }
  let value = fromBytesI32(<StaticArray<u8>>countData).unwrap();
  CL.ret(CLValue.fromI32(value));
}

export function call(): void {
  // Create entry points to get the counter value and to increment the counter by 1.
  let counterEntryPoints = new CL.EntryPoints();

  let entryPointInc = new CL.EntryPoint(COUNTER_INC, new Array(), new CLType(CLTypeTag.Unit), new CL.PublicAccess(), CL.EntryPointType.Contract);
  counterEntryPoints.addEntryPoint(entryPointInc);
  let entryPointGet = new CL.EntryPoint(COUNTER_GET, new Array(), new CLType(CLTypeTag.I32), new CL.PublicAccess(), CL.EntryPointType.Contract);
  counterEntryPoints.addEntryPoint(entryPointGet);

  // Initialize counter to 0.
  let counterLocalKey = Key.create(CLValue.fromI32(0));
  if (!counterLocalKey) {
    Error.fromUserError(0).revert();
    return;
  }

  // Create initial named keys of the contract.
  let counterNamedKeys = new Array<Pair<String, Key>>();
  counterNamedKeys.push(new Pair<String, Key>(COUNT_KEY, counterLocalKey));

  const result = CL.newContract(
    counterEntryPoints,
    counterNamedKeys,
    "counter_package_name",
    "counter_access_uref",
  );

  // To create a locked contract instead, use new_locked_contract and throw away the contract version returned
  // const result = CL.newLockedContract(counterEntryPoints, counterNamedKeys, "counter_package_name", "counter_access_uref");

  // The current version of the contract will be reachable through named keys
  const versionURef = Key.create(CLValue.fromI32(result.contractVersion));
  if (!versionURef) {
    return;
  }
  CL.putKey(CONTRACT_VERSION_KEY, versionURef);

  // Hash of the installed contract will be reachable through named keys
  CL.putKey(COUNTER_KEY, Key.fromHash(result.contractHash));
}
