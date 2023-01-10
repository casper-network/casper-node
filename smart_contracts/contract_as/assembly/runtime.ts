import * as externals from "./externals";
import { Error } from "./error";
import { typedToArray } from "./utils";

const BLAKE2B_DIGEST_LENGTH: i32 = 32;
const RANDOM_BYTES_COUNT: usize = 32;

/**
 * Performs a blake2b hash using a host function.
 * @param data Input bytes
 */
export function blake2b(data: Array<u8>): Array<u8> {
    let hashBytes = new Array<u8>(BLAKE2B_DIGEST_LENGTH);
    const ret = externals.casper_blake2b(data.dataStart, data.length, hashBytes.dataStart, hashBytes.length);
    let error = Error.fromResult(ret);
    if (error != null) {
        error.revert();
    }
    return hashBytes;
}

/**
 * Returns 32 pseudo random bytes.
 */
export function randomBytes(): Uint8Array {
    let randomBytes = new Uint8Array(RANDOM_BYTES_COUNT);
    const ret = externals.casper_random_bytes(randomBytes.dataStart, randomBytes.length);
    let error = Error.fromResult(ret);
    if (error != null) {
        error.revert();
    }
    return randomBytes;
}
