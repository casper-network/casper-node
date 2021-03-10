import * as externals from "./externals";
import {URef, AccessRights} from "./uref";
import {Error, ErrorCode} from "./error";
import {CLValue, CLType, CLTypeTag} from "./clvalue";
import {Key, AccountHash} from "./key";
import {Pair} from "./pair";
import {Ref} from "./ref";
import {toBytesString,
        toBytesVecT,
        fromBytesMap,
        fromBytesString,
        toBytesStringList,
        Result,
        toBytesMap,
        toBytesVecT,
        fromBytesArray} from "./bytesrepr";
import {KEY_UREF_SERIALIZED_LENGTH, UREF_ADDR_LENGTH, KEY_HASH_LENGTH} from "./constants";
import {RuntimeArgs} from "./runtime_args";
import {encodeUTF8} from "./utils";
import {Option} from "./option";

// NOTE: interfaces aren't supported in AS yet: https://github.com/AssemblyScript/assemblyscript/issues/146#issuecomment-399130960
// interface ToBytes {
//   fromBytes(bytes: Uint8Array): ToBytes;
// }

/**
 * Length of address
 */
const ADDR_LENGTH = 32;

/**
 * System contract types.
 */
export const enum SystemContract {
  /**
   * Mint contract.
   */
  Mint = 0,
  /**
   * Handle Payment contract.
   */
  HandlePayment = 1,
  /**
   * Standard Payment contract.
   */
  StandardPayment = 2,
  /**
   * Auction contract.
   */
  Auction = 3,
}

/**
 * Returns size in bytes of I-th parameter
 *
 * @internal
 * @param i I-th parameter
 */
export function getNamedArgSize(name: String): Ref<U32> | null {
  let size = new Array<u32>(1);
  size[0] = 0;

  const nameBuf = encodeUTF8(name);
  let ret = externals.get_named_arg_size(nameBuf.dataStart, nameBuf.length, size.dataStart);
  const error = Error.fromResult(ret);
  if (error !== null) {
    if (error.value() == ErrorCode.MissingArgument) {
      return null;
    }
    error.revert();
    return <Ref<U32>>unreachable();
  }
  const sizeU32 = changetype<U32>(size[0]);
  return new Ref<U32>(sizeU32);
}

/**
 * Returns the i-th argument passed to the host for the current module
 * invocation.
 *
 * Note that this is only relevant to contracts stored on-chain since a
 * contract deployed directly is not invoked with any arguments.
 *
 * @param i I-th parameter
 * @returns Array of bytes with ABI serialized argument. A null value if
 * given parameter is not present.
 */
export function getNamedArg(name: String): Uint8Array {
  let arg_size = getNamedArgSize(name);
  if (arg_size == null) {
    Error.fromErrorCode(ErrorCode.MissingArgument).revert();
    return <Uint8Array>unreachable();
  }
  let nameBytes = encodeUTF8(name);

  let arg_size_u32 = changetype<u32>(arg_size.value);
  let data = new Uint8Array(arg_size_u32);
  let ret = externals.get_named_arg(nameBytes.dataStart, nameBytes.length, data.dataStart, arg_size_u32);
  const error = Error.fromResult(ret);
  if (error !== null) {
    error.revert();
    return <Uint8Array>unreachable();
  }
  return data;
}

/**
 * Reads a given amount of bytes from a host buffer
 *
 * @internal
 * @param count Number of bytes
 * @returns A byte array with bytes received, otherwise a null in case of
 * errors.
 */
export function readHostBuffer(count: u32): Uint8Array {
  let result = new Uint8Array(count);
  let resultSize = new Uint32Array(1);

  let ret = externals.read_host_buffer(result.dataStart, result.length, resultSize.dataStart);
  const error = Error.fromResult(ret);
  if (error !== null) {
    error.revert();
    return <Uint8Array>unreachable();
  }
  return result;
}

/**
 * Returns an [[URef]] for a given system contract
 *
 * @internal
 * @param system_contract System contract variant
 * @returns A valid [[URef]] that points at system contract, otherwise null.
 */
function getSystemContract(systemContract: SystemContract): Uint8Array {
  let data = new Uint8Array(32);
  data[31] = <u8>systemContract + 1;
  return data;
}

/// Returns a read-only pointer to the Mint contract.
export function getMint(): Uint8Array {
  return getSystemContract(SystemContract.Mint);
}

/// Returns a read-only pointer to the Handle Payment contract.
export function getHandlePayment(): Uint8Array {
  return getSystemContract(SystemContract.HandlePayment);
}

/// Returns a read-only pointer to the Standard Payment contract.
export function getStandardPayment(): Uint8Array {
  return getSystemContract(SystemContract.StandardPayment);
}

/// Returns a read-only pointer to the Auction contract.
export function getAuction(): Uint8Array {
  return getSystemContract(SystemContract.Auction);
}



/**
 * Calls the given stored contract, passing the given arguments to it.
 *
 * If the stored contract calls [[ret]], then that value is returned from [[callContract]].  If the
 * stored contract calls [[Error.revert]], then execution stops and [[callContract]] doesn't return.
 * Otherwise [[callContract]] returns null.
 *
 * @param contractHash A key under which a contract is stored
 * @param args A list of values
 * @returns Bytes of the contract's return value.
 */
export function callContract(contractHash: Uint8Array, entryPointName: String, runtimeArgs: RuntimeArgs): Uint8Array {
  let argBytes = runtimeArgs.toBytes();
  let entryPointNameBytes = toBytesString(entryPointName);

  let resultSize = new Uint32Array(1);
  resultSize.fill(0);

  let ret = externals.call_contract(
      <usize>contractHash.dataStart,
      contractHash.length,
      entryPointNameBytes.dataStart,
      entryPointNameBytes.length,
      argBytes.dataStart,
      argBytes.length,
      resultSize.dataStart,
  );
  const error = Error.fromResult(ret);
  if (error !== null) {
    error.revert();
    return <Uint8Array>unreachable();
  }
  let hostBufSize = resultSize[0];
  if (hostBufSize > 0) {
    return readHostBuffer(hostBufSize);
  } else {
    return new Uint8Array(0);
  }
}

/**
 * Stores the given [[Key]] under a given name in the current context's named keys.
 *
 * The current context is either the caller's account or a stored contract
 * depending on whether the currently-executing module is a direct call or a
 * sub-call respectively.
 *
 * @category Runtime
 */
export function putKey(name: String, key: Key): void {
  var nameBytes = toBytesString(name);
  var keyBytes = key.toBytes();
  externals.put_key(
    nameBytes.dataStart,
    nameBytes.length,
    keyBytes.dataStart,
    keyBytes.length
  );
}

/**
 * Removes the [[Key]] stored under `name` in the current context's named keys.
 *
 * The current context is either the caller's account or a stored contract depending on whether the
 * currently-executing module is a direct call or a sub-call respectively.
 *
 * @param name Name of the key in current context's named keys
 * @returns An instance of [[Key]] if it exists, or a `null` otherwise.
 */
export function getKey(name: String): Key | null {
  var nameBytes = toBytesString(name);
  let keyBytes = new Uint8Array(KEY_UREF_SERIALIZED_LENGTH);
  let resultSize = new Uint32Array(1);
  let ret =  externals.get_key(
      nameBytes.dataStart,
      nameBytes.length,
      keyBytes.dataStart,
      keyBytes.length,
      resultSize.dataStart,
  );
  const error = Error.fromResult(ret);
  if (error !== null) {
    if (error.value() == ErrorCode.MissingKey) {
      return null;
    }
    error.revert();
    return <Key>unreachable();
  }
  let key = Key.fromBytes(keyBytes.slice(0, <i32>resultSize[0])); // total guess
  return key.unwrap();
}

/**
 * Returns the given [[CLValue]] to the host, terminating the currently
 * running module.
 *
 * Note this function is only relevant to contracts stored on chain which are
 * invoked via [[callContract]] and can thus return a value to their caller.
 * The return value of a directly deployed contract is never used.
 */
export function ret(value: CLValue): void {
  const valueBytes = value.toBytes();
  externals.ret(
    valueBytes.dataStart,
    valueBytes.length
  );
  unreachable();
}

/**
 * Returns `true` if `name` exists in the current context's named keys.
 *
 * The current context is either the caller's account or a stored contract depending on whether the
 * currently-executing module is a direct call or a sub-call respectively.
 *
 * @param name Name of the key
 */
export function hasKey(name: String): bool {
  const nameBytes = toBytesString(name);
  let ret = externals.has_key(nameBytes.dataStart, nameBytes.length);
  return ret == 0;
}

/**
 * Returns the current block time.
 */
export function getBlockTime(): u64 {
  let bytes = new Uint64Array(1);
  externals.get_blocktime(bytes.dataStart);
  return <u64>bytes[0];
}

/**
 * Returns the caller of the current context, i.e. the [[AccountHash]] of the
 * account which made the deploy request.
 */
export function getCaller(): AccountHash {
  let outputSize = new Uint32Array(1);
  let ret = externals.get_caller(outputSize.dataStart);
  const error = Error.fromResult(ret);
  if (error !== null) {
    error.revert();
    return <AccountHash>unreachable();
  }
  const accountHashBytes = readHostBuffer(outputSize[0]);
  const accountHashResult = AccountHash.fromBytes(accountHashBytes);
  if (accountHashResult.hasError()) {
    Error.fromErrorCode(ErrorCode.Deserialize).revert();
    return <AccountHash>unreachable();
  }
  return accountHashResult.value;
}

/**
 * The phase in which a given contract is executing.
 */
export enum Phase {
  /**
   * Set while committing the genesis or upgrade configurations.
   */
  System = 0,
  /**
   * Set while executing the payment code of a deploy.
   */
  Payment = 1,
  /**
   * Set while executing the session code of a deploy.
   */
  Session = 2,
  /**
   * Set while finalizing payment at the end of a deploy.
   */
  FinalizePayment = 3,
}

/**
 * Returns the current [[Phase]].
 */
export function getPhase(): Phase {
  let bytes = new Uint8Array(1);
  externals.get_phase(bytes.dataStart);
  const phase = bytes[0];
  return <Phase>phase;
}

/**
 * Removes the [[Key]] stored under `name` in the current context's named keys.
 *
 * The current context is either the caller's account or a stored contract depending on whether the
 * currently-executing module is a direct call or a sub-call respectively.
 */
export function removeKey(name: String): void{
  var nameBytes = toBytesString(name);
  externals.remove_key(nameBytes.dataStart, nameBytes.length);
}

/**
 * Returns the named keys of the current context.
 *
 * The current context is either the caller's account or a stored contract depending on whether the
 * currently-executing module is a direct call or a sub-call respectively.
 *
 * @returns An array of String and [[Key]] pairs
 */
export function listNamedKeys(): Array<Pair<String, Key>> {
  let totalKeys = new Uint32Array(1);
  let resultSize = new Uint32Array(1);

  const res = externals.load_named_keys(totalKeys.dataStart, resultSize.dataStart);
  const error = Error.fromResult(res);
  if (error !== null) {
    error.revert();
    return <Array<Pair<String, Key>>>unreachable();
  }

  if (totalKeys[0] == 0) {
    return new Array<Pair<String, Key>>();
  }

  let mapBytes = readHostBuffer(resultSize[0]);
  let maybeMap = fromBytesMap<String, Key>(
    mapBytes,
    fromBytesString,
    Key.fromBytes);

  if (maybeMap.hasError()) {
    Error.fromErrorCode(ErrorCode.Deserialize).revert();
    return <Array<Pair<String, Key>>>unreachable();
  }
  return maybeMap.value;
}

const ENTRYPOINTACCESS_PUBLIC_TAG: u8 = 1;
const ENTRYPOINTACCESS_GROUPS_TAG: u8 = 2;

export class EntryPointAccess {
  constructor(public cachedBytes: Array<u8>) {}
  toBytes(): Array<u8> {
    return this.cachedBytes;
  }
}

export class PublicAccess extends EntryPointAccess {
  constructor() {
    super([ENTRYPOINTACCESS_PUBLIC_TAG]);
  }
};

export class GroupAccess extends EntryPointAccess {
  constructor(groups: String[]) {
    let bytes: Array<u8> = [ENTRYPOINTACCESS_GROUPS_TAG];
    bytes = bytes.concat(toBytesStringList(groups));
    super(bytes);
  }
};


export enum EntryPointType {
  Session = 0,
  Contract = 1,
}

export class EntryPoint {
  constructor(public name: String,
              public args: Array<Pair<String, CLType>>,
              public ret: CLType,
              public access: EntryPointAccess,
              public entry_point_type: EntryPointType) {}

  toBytes(): Array<u8> {
    let nameBytes = toBytesString(this.name);
    let toBytesCLType = function(clType: CLType): Array<u8> { return clType.toBytes(); };
    let argsBytes = toBytesMap(this.args, toBytesString, toBytesCLType);
    let retBytes = this.ret.toBytes();
    let accessBytes = this.access.toBytes();
    let entryPointTypeBytes: Array<u8> = [<u8>this.entry_point_type];
    return nameBytes.concat(argsBytes).concat(retBytes).concat(accessBytes).concat(entryPointTypeBytes);
  }
};

export class EntryPoints {
  entryPoints: Array<Pair<String, EntryPoint>> = new Array<Pair<String, EntryPoint>>();
  addEntryPoint(entryPoint: EntryPoint): void {
    this.entryPoints.push(new Pair(entryPoint.name, entryPoint));
  }
  toBytes(): Array<u8> {
    let toBytesEntryPoint = function(entryPoint: EntryPoint): Array<u8> { return entryPoint.toBytes(); };
    return toBytesMap(this.entryPoints, toBytesString, toBytesEntryPoint);
  }
}

/**
 * A two-value structure that holds the result of [[createContractPackageAtHash]].
 */
export class CreateContractPackageResult {
  constructor(public packageHash: Uint8Array, public accessURef: URef) {}
}

export function createContractPackageAtHash(): CreateContractPackageResult {
  let hashAddr = new Uint8Array(KEY_HASH_LENGTH);
  let urefAddr = new Uint8Array(UREF_ADDR_LENGTH);
  externals.create_contract_package_at_hash(hashAddr.dataStart, urefAddr.dataStart, false);
  return new CreateContractPackageResult(
    hashAddr,
    new URef(urefAddr, AccessRights.READ_ADD_WRITE),
  );
}

export function createLockedContractPackageAtHash(): CreateContractPackageResult {
  let hashAddr = new Uint8Array(KEY_HASH_LENGTH);
  let urefAddr = new Uint8Array(UREF_ADDR_LENGTH);
  externals.create_contract_package_at_hash(hashAddr.dataStart, urefAddr.dataStart, true);
  return new CreateContractPackageResult(
      hashAddr,
      new URef(urefAddr, AccessRights.READ_ADD_WRITE),
  );
}

export function newContract(entryPoints: EntryPoints, namedKeys: Array<Pair<String, Key>> | null = null ,hashName: String | null = null, urefName: String | null = null): AddContractVersionResult {
  let result = createContractPackageAtHash();
  if (hashName !== null) {
    putKey(<String>hashName, Key.fromHash(result.packageHash));
  }
  if (urefName !== null) {
    putKey(<String>urefName, Key.fromURef(result.accessURef));
  }

  if (namedKeys === null) {
    namedKeys = new Array<Pair<String, Key>>();
  }

  return addContractVersion(
    result.packageHash,
    entryPoints,
    namedKeys,
  );
}

export function newLockedContract(entryPoints: EntryPoints, namedKeys: Array<Pair<String, Key>> | null = null ,hashName: String | null = null, urefName: String | null = null): AddContractVersionResult {
  let result = createLockedContractPackageAtHash();
  if (hashName !== null) {
    putKey(<String>hashName, Key.fromHash(result.packageHash));
  }
  if (urefName !== null) {
    putKey(<String>urefName, Key.fromURef(result.accessURef));
  }

  if (namedKeys === null) {
    namedKeys = new Array<Pair<String, Key>>();
  }

  return addContractVersion(
      result.packageHash,
      entryPoints,
      namedKeys,
  );
}


export function callVersionedContract(packageHash: Uint8Array, contract_version: Option, entryPointName: String, runtimeArgs: RuntimeArgs): Uint8Array {
  let entryPointBytes = toBytesString(entryPointName);
  let argBytes = runtimeArgs.toBytes();
  let bytesWritten = new Uint32Array(1);
  let bytesContractVersion = contract_version.toBytes();

  let ret = externals.call_versioned_contract(
    packageHash.dataStart,
    packageHash.length,
    bytesContractVersion.dataStart,
    bytesContractVersion.length,
    entryPointBytes.dataStart,
    entryPointBytes.length,
    argBytes.dataStart,
    argBytes.length,
    bytesWritten.dataStart,
  );
  let err = Error.fromResult(ret);
  if (err !== null) {
    err.revert();
  }
  if (bytesWritten[0] == 0) {
    return new Uint8Array(0);
  }
  else {
    return readHostBuffer(bytesWritten[0]);
  }
}

// Container for a result of contract version.
// Used as a replacement of non-existing tuples.
export class AddContractVersionResult {
  constructor(public contractHash: Uint8Array, public contractVersion: u32) {}
}

// Add new contract version. Requires a package hash, entry points and named keys.
// Result
export function addContractVersion(packageHash: Uint8Array, entryPoints: EntryPoints, namedKeys: Array<Pair<String, Key>>): AddContractVersionResult {
  var versionPtr = new Uint32Array(1);
  let entryPointsBytes = entryPoints.toBytes();
  let keyToBytes = function(key: Key): Array<u8> { return key.toBytes(); };
  let namedKeysBytes = toBytesMap(namedKeys, toBytesString, keyToBytes);
  let keyBytes = new Uint8Array(32);
  let totalBytes = new Uint32Array(1);

  let ret = externals.add_contract_version(
    packageHash.dataStart,
    packageHash.length,
    versionPtr.dataStart, // output
    entryPointsBytes.dataStart,
    entryPointsBytes.length,
    namedKeysBytes.dataStart,
    namedKeysBytes.length,
    keyBytes.dataStart,
    keyBytes.length,
    totalBytes.dataStart,
  );
  const error = Error.fromResult(ret);
  if (error !== null) {
    error.revert();
    return <AddContractVersionResult>unreachable();
  }

  const contractHash = keyBytes.slice(0, totalBytes[0]);
  const contractVersion = versionPtr[0];
  return new AddContractVersionResult(contractHash, contractVersion);
}

export function createContractUserGroup(packageHash: Uint8Array, label: String, newURefs: u8, existingURefs: Array<URef>): Array<URef> {
  let labelBytes = toBytesString(label);

  // NOTE: AssemblyScript sometimes is fine with closures, and sometimes
  // it generates unreachable code. Anonymous functions seems to be working
  // consistently.
  let toBytesURef = function(item: URef): Array<u8> { return item.toBytes(); }
  let fromBytesURef = function(bytes: Uint8Array): Result<URef> { return URef.fromBytes(bytes); }

  let existingUrefBytes: Array<u8> = toBytesVecT(existingURefs, toBytesURef);

  let outputSize = new Uint32Array(1);

  let ret = externals.create_contract_user_group(
    packageHash.dataStart,
    packageHash.length,
    labelBytes.dataStart,
    labelBytes.length,
    newURefs,
    existingUrefBytes.dataStart,
    existingUrefBytes.length,
    outputSize.dataStart,
  );

  let err = Error.fromResult(ret);
  if (err !== null) {
    err.revert();
    return <Array<URef>>unreachable();
  }
  let bytes = readHostBuffer(outputSize[0]);
  return fromBytesArray<URef>(bytes, fromBytesURef).unwrap();
}

export function removeContractUserGroup(
  packageHash: Uint8Array,
  label: String,
): void {
  let label_bytes = toBytesString(label);
  let ret = externals.remove_contract_user_group(
    packageHash.dataStart,
    packageHash.length,
    label_bytes.dataStart,
    label_bytes.length,
  );
  let err = Error.fromResult(ret);
  if (err !== null) {
    err.revert();
  }
}

export function extendContractUserGroupURefs(
  packageHash: Uint8Array,
  label: String,
): URef {
  let label_bytes = toBytesString(label);
  let size = new Uint32Array(1);
  let ret = externals.provision_contract_user_group_uref(
    packageHash.dataStart,
    packageHash.length,
    label_bytes.dataStart,
    label_bytes.length,
    size.dataStart,
  );
  let err = Error.fromResult(ret);
  if (err !== null) {
    err.revert();
  }
  let bytes = readHostBuffer(size[0]);
  return URef.fromBytes(bytes).unwrap();
}

export function removeContractUserGroupURefs(
  packageHash: Uint8Array,
  label: String,
  urefs: Array<URef>): void {

  let label_bytes = toBytesString(label);

  let encode = function(item: URef): Array<u8> { return item.toBytes(); };
  let urefsData = toBytesVecT(urefs, encode);

  let ret = externals.remove_contract_user_group_urefs(
    packageHash.dataStart,
    packageHash.length,
    label_bytes.dataStart,
    label_bytes.length,
    urefsData.dataStart,
    urefsData.length,
  );
  let err = Error.fromResult(ret);
  if (err !== null) {
    err.revert();
  }
}