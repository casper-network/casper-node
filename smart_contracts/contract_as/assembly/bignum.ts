import {Ref} from "./ref";
import {Error, Result} from "./bytesrepr";
import {Pair} from "./pair";

const HEX_LOWERCASE: string[] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'];

/**
 * Fast lookup of ascii character into it's numerical value in base16
 */
const HEX_DIGITS: i32[] =
[ -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
   0, 1, 2, 3, 4, 5, 6, 7, 8, 9,-1,-1,-1,-1,-1,-1,
  -1,0xa,0xb,0xc,0xd,0xe,0xf,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,0xa,0xb,0xc,0xd,0xe,0xf,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
  -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1 ];

/**
 * An implementation of 512-bit unsigned integers.
 */
export class U512 {
    private pn: Uint32Array;

    /**
     * Constructs a new instance of U512.
     */
    constructor() {
        this.pn = new Uint32Array(16); // 512 bits total
    }

    /**
     * @returns The maximum possible value of a U512.
     */
    static get MAX_VALUE(): U512 {
        let value = new U512();
        value.pn.fill(0xffffffff);
        return value;
    }

    /**
     * @returns The minimum possible value of a U512 (which is 0).
     */
    static get MIN_VALUE(): U512 {
        return new U512();
    }

    /**
     * Constructs a new U512 from a string of hex digits.
     */
    static fromHex(hex: String): U512 {
        let res = new U512();
        res.setHex(hex);
        return res;
    }

    /**
     * Converts a 64-bit unsigned integer into a U512.
     *
     * @param value The value to be converted.
     */
    static fromU64(value: u64): U512 {
        let res = new U512();
        res.setU64(value);
        return res;
    }

    /**
     * Gets the width of the number in bytes.
     */
    get width(): i32 {
        return this.pn.length * 4;
    }

    /**
     * Sets the value of this U512 to a given 64-bit value.
     *
     * @param value The desired new value of this U512.
     */
    setU64(value: u64): void {
        this.pn.fill(0);
        assert(this.pn.length >= 2);
        this.pn[0] = <u32>(value & <u64>0xffffffff);
        this.pn[1] = <u32>(value >> 32);
    }

    /**
     * Sets the value of this U512 to a value represented by the given string of hex digits.
     *
     * @param value The string of hex digits representing the desired value.
     */
    setHex(value: String): void {
        if (value.length >= 2 && value[0] == '0' && (value[1] == 'x' || value[1] == 'X'))
            value = value.substr(2);

        // Find the length
        let digits = 0;
        while (digits < value.length && HEX_DIGITS[value.charCodeAt(digits)] != -1 ) {
            digits++;
        }

        // Decodes hex string into an array of bytes
        let bytes = new StaticArray<u8>(this.width);

        // Convert ascii codes into values
        let i = 0;
        while (digits > 0 && i < bytes.length) {
            bytes[i] = <u8>HEX_DIGITS[value.charCodeAt(--digits)];

            if (digits > 0) {
                bytes[i] |= <u8>HEX_DIGITS[value.charCodeAt(--digits)] << 4;
                i++;
            }
        }

        // Reinterpret individual bytes back to u32 array
        this.setBytesLE(bytes);
    }

    /**
     * Checks whether this U512 is equal to 0.
     *
     * @returns True if this U512 is 0, false otherwise.
     */
    isZero(): bool {
        for (let i = 0; i < this.pn.length; i++) {
            if (this.pn[i] != 0) {
                return false;
            }
        }
        return true;
    }

    /**
     * The addition operator - adds two U512 numbers together.
     */
    @operator("+")
    add(other: U512): U512 {
        assert(this.pn.length == other.pn.length);
        // We do store carry as u64, to easily detect the overflow.
        let carry = <u64>0;
        for (let i = 0; i < this.pn.length; i++) {
            let n = carry + <u64>this.pn[i] + <u64>other.pn[i];
            // The actual value after (possibly) overflowing addition
            this.pn[i] = <u32>(n & <u64>0xffffffff);
            // Anything above 2^32-1 is the overflow and its what we carry over
            carry = <u64>(n >> 32);
        }
        return this;
    }

    /**
     * The negation operator - returns the two's complement of the argument.
     */
    @operator.prefix("-")
    neg(): U512 {
        let ret = new U512();
        for (let i = 0; i < this.pn.length; i++) {
            ret.pn[i] = ~this.pn[i];
        }
        ++ret;
        return ret;
    }

    /**
     * The subtraction operator - subtracts the two U512s.
     */
    @operator("-")
    sub(other: U512): U512 {
        return this.add(-other);
    }

    /**
     * The multiplication operator - calculates the product of the two arguments.
     */
    @operator("*")
    mul(other: U512): U512 {
        assert(this.pn.length == other.pn.length);
        let ret = new U512();
        // A naive implementation of multiplication
        for (let j = 0; j < this.pn.length; j++) {
            let carry: u64 = <u64>0;
            for (let i = 0; i + j < this.pn.length; i++) {
                // In a similar fashion to addition, we do arithmetic on 64 bit integers to detect overflow
                let n: u64 = carry + <u64>ret.pn[i + j] + <u64>this.pn[j] * <u64>other.pn[i];
                ret.pn[i + j] = <u32>(n & <u64>0xffffffff);
                carry = <u64>(n >> 32);
            }
        }
        return ret;
    }

    /**
     * Increments this U512.
     */
    private increment(): void {
        let i = 0;
        while (i < this.pn.length && ++this.pn[i] == 0) {
            i++;
        }
    }

    /**
     * Decrements this U512.
     */
    private decrement(): void {
        let i = 0;
        while (i < this.pn.length && --this.pn[i] == u32.MAX_VALUE) {
            i++;
        }
    }

    /**
     * Prefix operator `++` - increments this U512.
     */
    @operator.prefix("++")
    prefixInc(): U512 {
        this.increment();
        return this;
    }

    /**
     * Prefix operator `--` - decrements this U512.
     */
    @operator.prefix("--")
    prefixDec(): U512 {
        this.decrement();
        return this;
    }

    /**
     * Postfix operator `++` - increments this U512.
     */
    @operator.postfix("++")
    postfixInc(): U512 {
        let cloned = this.clone();
        cloned.increment();
        return cloned;
    }

    /**
     * Postfix operator `--` - decrements this U512.
     */
    @operator.postfix("--")
    postfixDec(): U512 {
        let cloned = this.clone();
        cloned.decrement();
        return cloned;
    }

    /**
     * Sets the values of the internally kept 32-bit "digits" (or "limbs") of the U512.
     *
     * @param pn The array of unsigned 32-bit integers to be used as the "digits"/"limbs".
     */
    setValues(pn: Uint32Array): void {
        for (let i = 0; i < this.pn.length; i++) {
            this.pn[i] = pn[i];
        }
    }

    /**
     * Clones the U512.
     */
    clone(): U512 {
        let U512val = new U512();
        U512val.setValues(this.pn);
        return U512val;
    }

    /**
     * Returns length of the integer in bits (not counting the leading zero bits).
     */
    bits(): u32 {
        for (let i = this.pn.length - 1; i >= 0; i--) {
            if (this.pn[i] > 0) {
                // Counts leading zeros
                return 32 * i + (32 - clz(this.pn[i]));
            }
        }
        return 0;
    }

    /**
     * Performs the integer division of `this/other`.
     *
     * @param other The divisor.
     * @returns A pair consisting of the quotient and the remainder, or null if the divisor was 0.
     */
    divMod(other: U512): Pair<U512, U512> | null {
        assert(this.pn.length == other.pn.length);

        let div = other.clone(); // make a copy, so we can shift.
        let num = this.clone(); // make a copy, so we can subtract the quotient.

        let res = new U512();

        let numBits = num.bits();
        let divBits = div.bits();

        if (divBits == 0) {
            // division by zero
            return null;
        }

        if (divBits > numBits) {
            // the result is certainly 0 and rem is the lhs of equation.
            let zero = new U512();
            return new Pair<U512, U512>(zero, num);
        }

        let shift: i32 = numBits - divBits;
        div <<= shift; // shift so that div and num align.

        while (shift >= 0) {
            if (num >= div) {
                num -= div;
                res.pn[shift / 32] |= (1 << (shift & 31)); // set a bit of the result.
            }
            div >>= 1; // shift back.
            shift--;
        }
        // num now contains the remainder of the division.
        return new Pair<U512, U512>(res, num);
    }

    /**
     * The division operator - divides the arguments.
     */
    @operator("/")
    div(other: U512): U512 {
        let divModResult = this.divMod(other);
        assert(divModResult !== null);
        return (<Pair<U512, U512>>divModResult).first;
    }

    /**
     * The 'modulo' operator - calculates the remainder from the division of the arguments.
     */
    @operator("%")
    rem(other: U512): U512 {
        let divModResult = this.divMod(other);
        assert(divModResult !== null);
        return (<Pair<U512, U512>>divModResult).second;
    }

    /**
     * The bitwise left-shift operator.
     */
    @operator("<<")
    shl(shift: u32): U512 {
        let res = new U512();

        let k: u32 = shift / 32;
        shift = shift % 32;

        for (let i = 0; i < this.pn.length; i++) {
            if (i + k + 1 < this.pn.length && shift != 0) {
                res.pn[i + k + 1] |= (this.pn[i] >> (32 - shift));
            }
            if (i + k < this.pn.length) {
                res.pn[i + k] |= (this.pn[i] << shift);
            }
        }

        return res;
    }

    /**
     * The bitwise right-shift operator.
     */
    @operator(">>")
    shr(shift: u32): U512 {
        let res = new U512();

        let k = shift / 32;
        shift = shift % 32;

        for (let i = 0; i < this.pn.length; i++) {
            if (i - k - 1 >= 0 && shift != 0) {
                res.pn[i - k - 1] |= (this.pn[i] << (32 - shift));
            }
            if (i - k >= 0) {
                res.pn[i - k] |= (this.pn[i] >> shift);
            }
        }

        return res;
    }

    /**
     * Compares `this` and `other`.
     *
     * @param other The number to compare `this` to.
     * @returns -1 if `this` is less than `other`, 1 if `this` is greater than `other`, 0 if `this`
     *     and `other` are equal.
     */
    cmp(other: U512): i32 {
        assert(this.pn.length == other.pn.length);
        for (let i = this.pn.length - 1; i >= 0; --i) {
            if (this.pn[i] < other.pn[i]) {
                return -1;
            }
            if (this.pn[i] > other.pn[i]) {
                return 1;
            }
        }
        return 0;
    }

    /**
     * The equality operator.
     *
     * @returns True if `this` and `other` are equal, false otherwise.
     */
    @operator("==")
    eq(other: U512): bool {
        return this.cmp(other) == 0;
    }

    /**
     * The not-equal operator.
     *
     * @returns False if `this` and `other` are equal, true otherwise.
     */
    @operator("!=")
    neq(other: U512): bool {
        return this.cmp(other) != 0;
    }

    /**
     * The greater-than operator.
     *
     * @returns True if `this` is greater than `other`, false otherwise.
     */
    @operator(">")
    gt(other: U512): bool {
        return this.cmp(other) == 1;
    }

    /**
     * The less-than operator.
     *
     * @returns True if `this` is less than `other`, false otherwise.
     */
    @operator("<")
    lt(other: U512): bool {
        return this.cmp(other) == -1;
    }

    /**
     * The greater-than-or-equal operator.
     *
     * @returns True if `this` is greater than or equal to `other`, false otherwise.
     */
    @operator(">=")
    gte(other: U512): bool {
        return this.cmp(other) >= 0;
    }

    /**
     * The less-than-or-equal operator.
     *
     * @returns True if `this` is less than or equal to `other`, false otherwise.
     */
    @operator("<=")
    lte(other: U512): bool {
        return this.cmp(other) <= 0;
    }

    /**
     * Returns a little-endian byte-array representation of this U512 (i.e. `result[0]` is the least
     * significant byte.
     */
    toBytesLE(): Uint8Array {
        let bytes = new Uint8Array(this.width);
        // Copy array of u32 into array of u8
        for (let i = 0; i < this.pn.length; i++) {
            store<u32>(bytes.dataStart + (i * 4), this.pn[i]);
        }
        return bytes;
    }

    /**
     * Sets the value of this U512 to the value represented by `bytes` when treated as a
     * little-endian representation of a number.
     */
    setBytesLE(bytes: StaticArray<u8>): void {
        let ptr = changetype<usize>(bytes);
        for (let i = 0; i < this.pn.length; i++) {
            let num = load<u32>(ptr + (i * 4));
            this.pn[i] = num;
        }
    }

    /**
     * Returns a string of hex digits representing the value of this U512.
     */
    private toHex(): String {
        let bytes = this.toBytesLE();
        let result = "";

        // Skips zeros in the back to make the numbers readable without tons of zeros in front
        let backZeros = bytes.length - 1;

        while (backZeros >= 0 && bytes[backZeros--] == 0) {}

        // First digit could be still 0 so skip it
        let firstByte = bytes[++backZeros];
        if ((firstByte & 0xF0) == 0) {
            // Skips the hi byte if the first character of the output base16 would be `0`
            // This way the hex string wouldn't be something like "01"
            result += HEX_LOWERCASE[firstByte & 0x0F];
        }
        else {
            result += HEX_LOWERCASE[firstByte >> 4];
            result += HEX_LOWERCASE[firstByte & 0x0F];
        }

        // Convert the rest of bytes into base16
        for (let i = backZeros - 1; i >= 0; i--) {
            let value = bytes[i];
            result += HEX_LOWERCASE[value >> 4];
            result += HEX_LOWERCASE[value & 0x0F];
        }
        return result;
    }

    /**
     * An alias for [[toHex]].
     */
    toString(): String {
        return this.toHex();
    }

    /**
     * Deserializes a U512 from an array of bytes. The array should represent a correct U512 in the
     * Casper serialization format.
     *
     * @returns A [[Result]] that contains the deserialized U512 if the deserialization was
     *    successful, or an error otherwise.
     */
    static fromBytes(bytes: StaticArray<u8>): Result<U512> {
        if (bytes.length < 1) {
            return new Result<U512>(null, Error.EarlyEndOfStream, 0);
        }

        const lengthPrefix = <i32>bytes[0];
        if (lengthPrefix > bytes.length) {
            return new Result<U512>(null, Error.EarlyEndOfStream, 0);
        }


        let res = new U512();

        // Creates a buffer so individual bytes can be placed there
        let buffer = new StaticArray<u8>(res.width);
        for (let i = 0; i < lengthPrefix; i++) {
            buffer[i] = bytes[i + 1];
        }

        res.setBytesLE(buffer);
        let ref = new Ref<U512>(res);
        return new Result<U512>(ref, Error.Ok, 1 + lengthPrefix);
    }

    /**
     * Serializes the U512 into an array of bytes that represents it in the Casper serialization
     * format.
     */
    toBytes(): Array<u8> {
        let bytes = this.toBytesLE();
        let skipZeros = bytes.length - 1;

        // Skip zeros at the end
        while (skipZeros >= 0 && bytes[skipZeros] == 0) {
            skipZeros--;
        }

        // Continue
        let lengthPrefix = skipZeros + 1;

        let result = new Array<u8>(1 + lengthPrefix);
        result[0] = <u8>lengthPrefix;
        for (let i = 0; i < lengthPrefix; i++) {
            result[1 + i] = bytes[i];
        }
        return result;
    }
};
