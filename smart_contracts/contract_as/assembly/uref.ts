import {Ref} from "./ref";
import {Error, Result} from "./bytesrepr";
import {UREF_ADDR_LENGTH} from "./constants";
import {checkTypedArrayEqual, typedToArray} from "./utils";
import {is_valid_uref, revert} from "./externals";

/**
 * A set of bitflags that defines access rights associated with a [[URef]].
 */
export enum AccessRights{
    /**
     * No permissions
     */
    NONE = 0x0,
    /**
     * Permission to read the value under the associated [[URef]].
     */
    READ = 0x1,
    /**
     * Permission to write a value under the associated [[URef]].
     */
    WRITE = 0x2,
    /**
     * Permission to read or write the value under the associated [[URef]].
     */
    READ_WRITE = 0x3,
    /**
     * Permission to add to the value under the associated [[URef]].
     */
    ADD = 0x4,
    /**
     * Permission to read or add to the value under the associated [[URef]].
     */
    READ_ADD = 0x5,
    /**
     * Permission to add to, or write the value under the associated [[URef]].
     */
    ADD_WRITE = 0x6,
    /**
     * Permission to read, add to, or write the value under the associated [[URef]].
     */
    READ_ADD_WRITE = 0x07,
}

/**
 * Represents an unforgeable reference, containing an address in the network's global storage and
 * the [[AccessRights]] of the reference.
 *
 * A [[URef]] can be used to index entities such as [[CLValue]]s, or smart contracts.
 */
export class URef {
    /**
     * Representation of URef address.
     */
    private bytes: Uint8Array;
    private accessRights: AccessRights

    /**
     * Constructs new instance of URef.
     * @param bytes Bytes representing address of the URef.
     * @param accessRights Access rights flag. Use [[AccessRights.NONE]] to indicate no permissions.
     */
    constructor(bytes: Uint8Array, accessRights: AccessRights) {
        this.bytes = bytes;
        this.accessRights = accessRights;
    }

    /**
     * Returns the address of this URef as an array of bytes.
     *
     * @returns A byte array with a length of 32.
     */
    public getBytes(): Uint8Array {
        return this.bytes;
    }

    /**
     * Returns the access rights of this [[URef]].
     */
    public getAccessRights(): AccessRights {
        return this.accessRights;
    }

    /**
     * Sets the access rights of this [[URef]].
     */
    public setAccessRights(newAccessRights: AccessRights): void {
        this.accessRights = newAccessRights;
    }

    /**
     * Returns new [[URef]] with modified access rights.
     */
      public withAccessRights(newAccessRights: AccessRights): URef {
        return new URef(this.bytes, newAccessRights);
    }

    /**
     * Validates uref against named keys.
     */
    public isValid(): boolean{
        const urefBytes = this.toBytes();
        let ret = is_valid_uref(
            urefBytes.dataStart,
            urefBytes.length
        );
        return ret !== 0;
    }

    /**
     * Deserializes a new [[URef]] from bytes.
     * @param bytes Input bytes. Requires at least 33 bytes to properly deserialize an [[URef]].
     */
    static fromBytes(bytes: Uint8Array): Result<URef> {
        if (bytes.length < 33) {
            return new Result<URef>(null, Error.EarlyEndOfStream, 0);
        }

        let urefBytes = bytes.subarray(0, UREF_ADDR_LENGTH);
        let currentPos = 33;

		let accessRights = bytes[UREF_ADDR_LENGTH];
		let uref = new URef(urefBytes, accessRights);
		let ref = new Ref<URef>(uref);
		return new Result<URef>(ref, Error.Ok, currentPos);
    }

    /**
     * Serializes the URef into an array of bytes that represents it in the Casper serialization
     * format.
     */
    toBytes(): Array<u8> {
        let result = typedToArray(this.bytes);
        result.push(<u8>this.accessRights);
        return result;
    }

    /**
     * The equality operator.
     *
     * @returns True if `this` and `other` are equal, false otherwise.
     */
    @operator("==")
    equalsTo(other: URef): bool {
        return checkTypedArrayEqual(this.bytes, other.bytes) && this.accessRights == other.accessRights;
    }

    /**
     * The not-equal operator.
     *
     * @returns False if `this` and `other` are equal, true otherwise.
     */
    @operator("!=")
    notEqualsTo(other: URef): bool {
        return !this.equalsTo(other);
    }
}
