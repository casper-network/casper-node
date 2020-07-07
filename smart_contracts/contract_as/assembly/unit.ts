/**
 * A class representing the unit type, i.e. a type that has no values (equivalent to eg. `void` in
 * C or `()` in Rust).
 */
export class Unit{
    /**
     * Serializes a [[Unit]] - returns an empty array.
     */
    toBytes(): Array<u8> {
        return new Array<u8>(0);
    }

    /**
     * Deserializes a [[Unit]] - returns a new [[Unit]].
     */
    static fromBytes(bytes: Uint8Array): Unit{
        return new Unit();
    }
}
