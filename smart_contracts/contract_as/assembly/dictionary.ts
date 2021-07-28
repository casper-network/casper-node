//@ts-nocheck
import * as externals from "./externals";
import {Error, ErrorCode} from "./error";
import {CLValue} from "./clvalue";
import {readHostBuffer} from "./index";
import { URef } from "./uref";
import { arrayToTyped, encodeUTF8 } from "./utils";
import { Key } from "./key";
import * as CL from "./";

/**
 * Creates new seed for a dictionary partition of the global state.
 * 
 * @category Storage
 * @returns Returns newly provisioned [[URef]]
 */
export function newDictionary(keyName: String): URef {
    if (keyName.length == 0 || CL.hasKey(keyName)) {
        Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
        return <URef>unreachable();
    }
    let outputSize = new Uint32Array(1);
    let ret = externals.casper_new_dictionary(outputSize.dataStart);
    const error = Error.fromResult(ret);
    if (error !== null) {
        error.revert();
        return <URef>unreachable();
    }
    let urefBytes = readHostBuffer(outputSize[0]);
    const uref = URef.fromBytes(urefBytes).unwrap();
    const key = Key.fromURef(uref);
    CL.putKey(keyName, key);
    return uref;
}

/**
 * Reads the value under `key` in the dictionary partition of global state.
 *
 * @category Storage
 * @returns Returns bytes of serialized value, otherwise a null if given dictionary does not exists.
 */
export function dictionaryGet(seed_uref: URef, key: String): Uint8Array | null {
    let seedUrefBytes = seed_uref.toBytes();
    const keyBytes = encodeUTF8(key);

    let valueSize = new Uint8Array(1);
    const ret = externals.dictionary_get(seedUrefBytes.dataStart, seedUrefBytes.length, keyBytes.dataStart, keyBytes.length, valueSize.dataStart);
    if (ret == ErrorCode.ValueNotFound){
        return null;
    }
    const error = Error.fromResult(ret);
    if (error != null) {
        error.revert();
        return <Uint8Array>unreachable();
    }
    return readHostBuffer(valueSize[0]);
}

/**
 * Writes `value` under `key` in a dictionary.
 * @category Storage
 */
export function dictionaryPut(uref: URef, key: String, value: CLValue): void {
    const keyBytes = encodeUTF8(key);
    const urefBytes = uref.toBytes();
    const valueBytes = value.toBytes();
    externals.dictionary_put(
        urefBytes.dataStart,
        urefBytes.length,
        keyBytes.dataStart,
        keyBytes.length,
        valueBytes.dataStart,
        valueBytes.length
    );
}
