const OPTION_TAG_NONE: u8 = 0;
const OPTION_TAG_SOME: u8 = 1;

// TODO: explore Option<T> (without interfaces to constrain T with, is it practical?)
/**
 * A class representing an optional value, i.e. it might contain either a value of some type or
 * no value at all. Similar to Rust's `Option` or Haskell's `Maybe`.
 */
export class Option{
    private bytes: Array<u8>  | null;

	/**
	 * Constructs a new option containing the value of `bytes`. `bytes` can be `null`, which
	 * indicates no value.
	 */
    constructor(bytes: Array<u8>  | null) {
        this.bytes = bytes;
    }

	/**
	 * Checks whether the `Option` contains no value.
	 *
	 * @returns True if the `Option` has no value.
	 */
    isNone(): bool{
        return this.bytes === null;
    }

	/**
	 * Checks whether the `Option` contains a value.
	 *
	 * @returns True if the `Option` has some value.
	 */
    isSome() : bool{
        return this.bytes != null;
    }

	/**
	 * Unwraps the `Option`, returning the inner value (or `null` if there was none).
	 *
	 * @returns The inner value, or `null` if there was none.
	 */
    unwrap(): Array<u8> {
        assert(this.isSome());
        return <Array<u8>>this.bytes;
    }

	/**
	 * Serializes the `Option` into an array of bytes.
	 */
    toBytes(): Array<u8>{
        if (this.bytes === null){
            let result = new Array<u8>(1);
            result[0] = OPTION_TAG_NONE;
            return result;
        }
        const bytes = <Array<u8>>this.bytes;

        let result = new Array<u8>(bytes.length + 1);
        result[0] = OPTION_TAG_SOME;
        for (let i = 0; i < bytes.length; i++) {
            result[i+1] = bytes[i];
        }

        return result;
    }

	/**
	 * Deserializes an array of bytes into an `Option`.
	 */
    static fromBytes(bytes: StaticArray<u8>): Option{
        // check SOME / NONE flag at head
        // TODO: what if length is exactly 1?
        if (bytes.length >= 1 && bytes[0] == 1)
            return new Option(bytes.slice(1));

        return new Option(null);
    }
}
