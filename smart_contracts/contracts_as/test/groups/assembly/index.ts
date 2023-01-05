//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { Key } from "../../../../contract_as/assembly/key";
import { URef } from "../../../../contract_as/assembly/uref";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { Pair } from "../../../../contract_as/assembly/pair";
import { RuntimeArgs } from "../../../../contract_as/assembly/runtime_args";
import { Option } from "../../../../contract_as/assembly/option";
import { toBytesU32 } from "../../../../contract_as/assembly/bytesrepr";
import { arrayToTyped } from "../../../../contract_as/assembly/utils";
import { U512 } from "../../../../contract_as/assembly/bignum";
import { transferFromPurseToPurse, transferFromPurseToPursePassthrough } from "../../../../contract_as/assembly/purse";
import { getMainPurse, getMainPurseBytes } from "../../../../contract_as/assembly/account";

const CONTRACT_INITIAL_VERSION: u8 = 1;
const PACKAGE_HASH_KEY = "package_hash_key";
const PACKAGE_ACCESS_KEY = "package_access_key";
const RESTRICTED_CONTRACT = "restricted_contract";
const RESTRICTED_SESSION = "restricted_session";
const RESTRICTED_SESSION_CALLER = "restricted_session_caller";
const UNRESTRICTED_CONTRACT_CALLER = "unrestricted_contract_caller";
const RESTRICTED_CONTRACT_CALLER_AS_SESSION = "restricted_contract_caller_as_session";
const UNCALLABLE_SESSION = "uncallable_session";
const UNCALLABLE_CONTRACT = "uncallable_contract";
const CALL_RESTRICTED_ENTRY_POINTS = "call_restricted_entry_points";
const ARG_PACKAGE_HASH = "package_hash";

export function restricted_session(): void { }

export function restricted_contract(): void { }

export function restricted_session_caller(): void {
  let packageHashBytes = CL.getNamedArg(ARG_PACKAGE_HASH);
  let packageHash = packageHashBytes.slice(1);
  let contractVersion = new Option(toBytesU32(CONTRACT_INITIAL_VERSION));
  CL.callVersionedContract(
    StaticArray.fromArray(packageHash),
    contractVersion,
    RESTRICTED_SESSION,
    new RuntimeArgs(),
  );
}

function contract_caller(): void {
  let packageHashBytes = CL.getNamedArg(ARG_PACKAGE_HASH);
  // let packageKey = Key.fromBytes(packageHashBytes).unwrap();
  let packageHash = packageHashBytes.slice(1);
  let contractVersion = new Option(toBytesU32(CONTRACT_INITIAL_VERSION));
  CL.callVersionedContract(
    StaticArray.fromArray(packageHash),
    contractVersion,
    RESTRICTED_CONTRACT,
    new RuntimeArgs(),
  );
}

export function unrestricted_contract_caller(): void {
  contract_caller();
}

export function restricted_contract_caller_as_session(): void {
  contract_caller();
}

export function uncallable_session(): void { }

export function uncallable_contract(): void { }

export function restricted_standard_payment(): void {
  // Performs standard payment operation as a restricted session entrypoint.
  let amountBytes = CL.getNamedArg("amount");
  if (amountBytes === null) {
    Error.fromErrorCode(ErrorCode.MissingArgument).revert();
    return;
  }
  let handlePayment = CL.getSystemContract(CL.SystemContract.HandlePayment);
  let paymentPurseBytes = CL.callContract(handlePayment, "get_payment_purse", null);
  if (paymentPurseBytes === null) {
    Error.fromErrorCode(ErrorCode.InvalidPurse).revert();
    return;
  }
  let err = transferFromPurseToPursePassthrough(getMainPurseBytes(), paymentPurseBytes, amountBytes);
  if (err !== null) {
    err.revert();
    return;
  }
}

export function call_restricted_entry_points(): void {
  // We're aggressively removing exports that aren't exposed through contract header so test
  // ensures that those exports are still inside WASM.
  uncallable_session();
  uncallable_contract();
}


function createGroup(packageHash: StaticArray<u8>): URef {
  let key = Key.create(CLValue.fromU64(0));
  if (key === null) {
    Error.fromErrorCode(ErrorCode.Formatting).revert();
    throw 0;
  }

  CL.putKey("saved_uref", key);

  let existingURefs: Array<URef> = [<URef>key.uref];

  let newURefs = CL.createContractUserGroup(
    packageHash,
    "Group 1",
    1,
    existingURefs,
  );

  if (newURefs.length != 1) {
    Error.fromUserError(4464 + 1000 + 1).revert();
    throw 0;
  }
  return newURefs[0];
}

function createEntryPoints(): CL.EntryPoints {
  let entryPoints = new CL.EntryPoints();
  let restrictedSession = new CL.EntryPoint(
    RESTRICTED_SESSION,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    new CL.GroupAccess(["Group 1"]),
    CL.EntryPointType.Session,
  );
  entryPoints.addEntryPoint(restrictedSession);

  let restricted_contract = new CL.EntryPoint(
    RESTRICTED_CONTRACT,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    new CL.GroupAccess(["Group 1"]),
    CL.EntryPointType.Contract,
  );
  entryPoints.addEntryPoint(restricted_contract);

  let restrictedSessionCallerParams = new Array<Pair<String, CLType>>();
  restrictedSessionCallerParams.push(new Pair(ARG_PACKAGE_HASH, new CLType(CLTypeTag.Key)));
  let restricted_session_caller = new CL.EntryPoint(
    RESTRICTED_SESSION_CALLER,
    restrictedSessionCallerParams,
    new CLType(CLTypeTag.Unit),
    new CL.PublicAccess(),
    CL.EntryPointType.Session,
  );
  entryPoints.addEntryPoint(restricted_session_caller);

  let restricted_contract2 = new CL.EntryPoint(
    RESTRICTED_CONTRACT,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    new CL.GroupAccess(["Group 1"]),
    CL.EntryPointType.Contract,
  );
  entryPoints.addEntryPoint(restricted_contract2);

  let unrestricted_contract_caller = new CL.EntryPoint(
    UNRESTRICTED_CONTRACT_CALLER,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    // Made public because we've tested deploy level auth into a contract in
    // RESTRICTED_CONTRACT entrypoint
    new CL.PublicAccess(),
    // NOTE: Public contract authorizes any contract call, because this contract has groups
    // uref in its named keys
    CL.EntryPointType.Contract,
  );
  entryPoints.addEntryPoint(unrestricted_contract_caller);

  let unrestricted_contract_caller_as_session = new CL.EntryPoint(
    RESTRICTED_CONTRACT_CALLER_AS_SESSION,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    // Made public because we've tested deploy level auth into a contract in
    // RESTRICTED_CONTRACT entrypoint
    new CL.PublicAccess(),
    // NOTE: Public contract authorizes any contract call, because this contract has groups
    // uref in its named keys
    CL.EntryPointType.Session,
  );
  entryPoints.addEntryPoint(unrestricted_contract_caller_as_session);

  let uncallable_session = new CL.EntryPoint(
    UNCALLABLE_SESSION,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    // Made public because we've tested deploy level auth into a contract in
    // RESTRICTED_CONTRACT entrypoint
    new CL.GroupAccess([]),
    // NOTE: Public contract authorizes any contract call, because this contract has groups
    // uref in its named keys
    CL.EntryPointType.Session,
  );
  entryPoints.addEntryPoint(uncallable_session);

  let uncallable_contract = new CL.EntryPoint(
    UNCALLABLE_CONTRACT,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    // Made public because we've tested deploy level auth into a contract in
    // RESTRICTED_CONTRACT entrypoint
    new CL.GroupAccess([]),
    // NOTE: Public contract authorizes any contract call, because this contract has groups
    // uref in its named keys
    CL.EntryPointType.Session,
  );
  entryPoints.addEntryPoint(uncallable_contract);

  let standardPaymentParams = new Array<Pair<String, CLType>>();
  standardPaymentParams.push(new Pair("amount", new CLType(CLTypeTag.U512)));
  let restrictedStandardPayment = new CL.EntryPoint(
    "restricted_standard_payment",
    standardPaymentParams,
    new CLType(CLTypeTag.Unit),
    // Made public because we've tested deploy level auth into a contract in
    // RESTRICTED_CONTRACT entrypoint
    new CL.GroupAccess(["Group 1"]),
    // NOTE: Public contract authorizes any contract call, because this contract has groups
    // uref in its named keys
    CL.EntryPointType.Session,
  );
  entryPoints.addEntryPoint(restrictedStandardPayment);

  // Directly calls entryPoints that are protected with empty group of lists to verify that even
  // though they're not callable externally, they're still visible in the WASM.
  let call_restricted_entry_points = new CL.EntryPoint(
    CALL_RESTRICTED_ENTRY_POINTS,
    new Array<Pair<String, CLType>>(),
    new CLType(CLTypeTag.Unit),
    // Made public because we've tested deploy level auth into a contract in
    // RESTRICTED_CONTRACT entrypoint
    new CL.PublicAccess(),
    // NOTE: Public contract authorizes any contract call, because this contract has groups
    // uref in its named keys
    CL.EntryPointType.Session,
  );
  entryPoints.addEntryPoint(call_restricted_entry_points);

  return entryPoints;
}

function installVersion1(
  contractPackageHash: StaticArray<u8>,
  restrictedURef: URef,
): void {
  let contractVariable = Key.create(CLValue.fromI32(0));
  if (!contractVariable) {
    Error.fromErrorCode(ErrorCode.Formatting).revert();
    return;
  }

  let namedKeys = new Array<Pair<String, Key>>();
  namedKeys.push(new Pair("contract_named_key", <Key>contractVariable));
  namedKeys.push(new Pair("restricted_uref", Key.fromURef(restrictedURef)));

  let entryPoints = createEntryPoints();

  const result = CL.addContractVersion(
    contractPackageHash,
    entryPoints,
    namedKeys,
  );
}


export function call(): void {
  let createResult = CL.createContractPackageAtHash();
  CL.putKey(PACKAGE_HASH_KEY, Key.fromHash(createResult.packageHash));
  CL.putKey(PACKAGE_ACCESS_KEY, Key.fromURef(createResult.accessURef));

  let restrictedURef = createGroup(createResult.packageHash);
  installVersion1(createResult.packageHash, restrictedURef);
}
