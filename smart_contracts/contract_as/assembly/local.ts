//@ts-nocheck
import * as externals from "./externals";
import {Error, ErrorCode} from "./error";
import {CLValue} from "./clvalue";
import {readHostBuffer} from "./index";

/**
 * Reads the value under `key` in the context-local partition of global state.
 *
 * @category Storage
 * @returns Returns bytes of serialized value, otherwise a null if given local key does not exists.
 */
export function readLocal(local: Uint8Array): Uint8Array | null {
    let valueSize = new Uint8Array(1);
    const ret = externals.read_local(local.dataStart, local.length, valueSize.dataStart);
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
export function writeLocal(local: Uint8Array, value: CLValue): void {
    const valueBytes = value.toBytes();
    externals.write_local(
        local.dataStart,
        local.length,
        valueBytes.dataStart,
        valueBytes.length
    );
}
