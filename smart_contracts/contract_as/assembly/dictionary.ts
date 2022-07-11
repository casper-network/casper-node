//@ts-nocheck
import * as externals from "./externals";
import { Error, ErrorCode } from "./error";
import { CLValue } from "./clvalue";
import { readHostBuffer } from "./index";
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
        throw 0;
    }
    let outputSize = new Uint32Array(1);
    let ret = externals.casper_new_dictionary(outputSize.dataStart);
    const error = Error.fromResult(ret);
    if (error !== null) {
        error.revert();
        throw 0;
    }
    let urefBytes = readHostBuffer(outputSize[0]);
    const uref = URef.fromBytes(urefBytes).unwrap();
    const urefKey = Key.fromURef(uref);
    CL.putKey(keyName, urefKey);
    return uref;
}

/**
 * Reads the value under `dictionary_item_key` in the dictionary partition of global state.
 *
 * @category Storage
 * @returns Returns bytes of serialized value, otherwise a null if given dictionary does not exists.
 */
export function dictionaryGet(seed_uref: URef, dictionary_item_key: String): Uint8Array | null {
    let seedUrefBytes = seed_uref.toBytes();
    const keyBytes = encodeUTF8(dictionary_item_key);

    let valueSize = new Uint8Array(1);
    const ret = externals.dictionary_get(seedUrefBytes.dataStart, seedUrefBytes.length, keyBytes.dataStart, keyBytes.length, valueSize.dataStart);
    if (ret == ErrorCode.ValueNotFound) {
        return null;
    }
    const error = Error.fromResult(ret);
    if (error != null) {
        error.revert();
        throw 0;
    }
    return readHostBuffer(valueSize[0]);
}

/**
 * Reads `value` under `dictionary-key` in a dictionary.
 * @category Storage
 */
export function dictionaryRead(dictionaryKey: Key): Uint8Array | null {
    const dictionaryKeyBytes = dictionaryKey.toBytes();
    let valueSize = new Uint8Array(1);
    const ret = externals.dictionary_read(
        dictionaryKeyBytes.dataStart,
        dictionaryKeyBytes.length,
        valueSize.dataStart
    )
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

