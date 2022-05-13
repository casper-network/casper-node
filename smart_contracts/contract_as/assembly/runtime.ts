import * as externals from "./externals";
import {Error} from "./error";

const ADDRESS_LENGTH: i32 = 32;
const BLAKE2B_DIGEST_LENGTH: i32 = 32;

/**
 * Performs a blake2b hash using a host function.
 * @param data Input bytes
 */
 export function blake2b(data: Array<u8>): Uint8Array {
    let hashBytes = new Uint8Array(BLAKE2B_DIGEST_LENGTH);
    const ret = externals.casper_blake2b(data.dataStart, data.length, hashBytes.dataStart, hashBytes.length);
    let error = Error.fromResult(ret);
    if(error != null){
        error.revert();
    }
    return hashBytes;
}

/**
 * Returns a 32-bytes long new address.
 */
export function nextAddress(): Uint8Array {
    let addressBytes = new Uint8Array(ADDRESS_LENGTH);
    const ret = externals.casper_next_address(addressBytes.dataStart, addressBytes.length);
    let error = Error.fromResult(ret);
    if (error != null) {
        error.revert();
    }
    return addressBytes;
}