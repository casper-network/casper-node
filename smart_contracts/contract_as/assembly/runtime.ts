import * as externals from "./externals";
import { Error } from "./error";
import { typedToArray } from "./utils";

const BLAKE2B_DIGEST_LENGTH: i32 = 32;

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
