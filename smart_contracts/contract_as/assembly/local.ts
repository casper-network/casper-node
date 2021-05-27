//@ts-nocheck
import * as externals from "./externals";
import {Error, ErrorCode} from "./error";
import {CLValue} from "./clvalue";
import {readHostBuffer} from "./index";
import { URef } from "./uref";
import { arrayToTyped } from "./utils";

/**
 * Creates new seed for context-local partition of the global state.
 * 
 * @category Storage
 * @returns Returns newly provisioned [[URef]]
 */
export function createLocal(): URef {
    let outputSize = new Uint32Array(1);
    let ret = externals.casper_new_dictionary(outputSize.dataStart);
    const error = Error.fromResult(ret);
    if (error !== null) {
        error.revert();
        return <URef>unreachable();
    }
    let urefBytes = readHostBuffer(outputSize[0]);
    return URef.fromBytes(urefBytes).unwrap();
}

/**
 * Reads the value under `key` in the dictionary partition of global state.
 *
 * @category Storage
 * @returns Returns bytes of serialized value, otherwise a null if given dictionary does not exists.
 */
export function dictionaryGet(seed_uref: URef, local: Array<u8>): Uint8Array | null {
    let seedUrefBytes = seed_uref.toBytes();
    const localBytes = arrayToTyped(local);

    let valueSize = new Uint8Array(1);
    const ret = externals.dictionary_get(seedUrefBytes.dataStart, seedUrefBytes.length, localBytes.dataStart, localBytes.length, valueSize.dataStart);
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
 * Writes `value` under `key` in the context-local partition of global state.
 * @category Storage
 */
export function dictionaryPut(uref: URef, local: Array<u8>, value: CLValue): void {
    const localBytes = arrayToTyped(local);
    const urefBytes = uref.toBytes();
    const valueBytes = value.toBytes();
    externals.dictionary_put(
        urefBytes.dataStart,
        urefBytes.length,
        localBytes.dataStart,
        localBytes.length,
        valueBytes.dataStart,
        valueBytes.length
    );
}
