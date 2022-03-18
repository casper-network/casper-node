import * as externals from "./externals";
import {readHostBuffer} from ".";
import {UREF_SERIALIZED_LENGTH} from "./constants";
import {URef} from "./uref";
import {CLValue} from "./clvalue";
import {Error, ErrorCode} from "./error";
import {checkTypedArrayEqual, typedToArray, encodeUTF8} from "./utils";
import {Ref} from "./ref";
import {Result, Error as BytesreprError} from "./bytesrepr";
import {PublicKey} from "./public_key";
import {RuntimeArgs} from "./runtime_args";
import * as runtime from "./runtime";

/**
 * Enum representing a variant of a [[Key]] - Account, Hash or URef.
 */
export enum KeyVariant {
    /** The Account variant */
    ACCOUNT_ID = 0,
    /** The Hash variant */
    HASH_ID = 1,
    /** The URef variant */
    UREF_ID = 2,
}

/** A cryptographic public key. */
export class AccountHash {
    /**
     * Constructs a new `AccountHash`.
     *
     * @param bytes The bytes constituting the public key.
     */
    constructor(public bytes: Uint8Array) {}

    /** Checks whether two `AccountHash`s are equal. */
    @operator("==")
    equalsTo(other: AccountHash): bool {
        return checkTypedArrayEqual(this.bytes, other.bytes);
    }

    /** Checks whether two `AccountHash`s are not equal. */
    @operator("!=")
    notEqualsTo(other: AccountHash): bool {
        return !this.equalsTo(other);
    }

    @operator(">")
    greaterThan(other: AccountHash): bool {
        for (let i = 0; i < 32; i++) {
            if (this.bytes[i] > other.bytes[i]) {
                return true;
            }
            if (this.bytes[i] < other.bytes[i]) {
                return false;
            }
        }
        return false;
    }

    @operator("<")
    lowerThan(other: AccountHash): bool {
        for (let i = 0; i < 32; i++) {
            if (this.bytes[i] < other.bytes[i]) {
                return true;
            }
            if (this.bytes[i] > other.bytes[i]) {
                return false;
            }
        }
        return false;
    }

    static fromPublicKey(publicKey: PublicKey) : AccountHash {
        let algorithmName = publicKey.getAlgorithmName();
        let algorithmNameBytes = encodeUTF8(algorithmName);
        let publicKeyBytes = publicKey.getRawBytes();
        let dataLength = algorithmNameBytes.length + publicKeyBytes.length + 1;

        let data = new Array<u8>(dataLength);
        for (let i=0; i < algorithmNameBytes.length; i++){
            data[i]=algorithmNameBytes[i];
        }

        data[algorithmNameBytes.length] = 0;

        for (let i=0; i < publicKeyBytes.length; i++) {
            data[algorithmNameBytes.length + 1 + i] = publicKeyBytes[i];
        }

        const accountHashBytes = runtime.blake2b(data);
        return new AccountHash(accountHashBytes);
    }


    /** Deserializes a `AccountHash` from an array of bytes. */
    static fromBytes(bytes: Uint8Array): Result<AccountHash> {
        if (bytes.length < 32) {
            return new Result<AccountHash>(null, BytesreprError.EarlyEndOfStream, 0);
        }

        let accountHashBytes = bytes.subarray(0, 32);
        let accountHash = new AccountHash(accountHashBytes);
        let ref = new Ref<AccountHash>(accountHash);
        return new Result<AccountHash>(ref, BytesreprError.Ok, 32);
    }

    /** Serializes a `AccountHash` into an array of bytes. */
    toBytes(): Array<u8> {
        return typedToArray(this.bytes);
    }
}

/**
 * The type under which data (e.g. [[CLValue]]s, smart contracts, user accounts)
 * are indexed on the network.
 */
export class Key {
    variant: KeyVariant;
    hash: Uint8Array | null;
    uref: URef | null;
    account: AccountHash | null;

    /** Creates a `Key` from a given [[URef]]. */
    static fromURef(uref: URef): Key {
        let key = new Key();
        key.variant = KeyVariant.UREF_ID;
        key.uref = uref;
        return key;
    }

    /** Creates a `Key` from a given hash. */
    static fromHash(hash: Uint8Array): Key {
        let key = new Key();
        key.variant = KeyVariant.HASH_ID;
        key.hash = hash;
        return key;
    }

    /** Creates a `Key` from a [[<AccountHash>]] representing an account. */
    static fromAccount(account: AccountHash): Key {
        let key = new Key();
        key.variant = KeyVariant.ACCOUNT_ID;
        key.account = account;
        return key;
    }

    /**
     * Attempts to write `value` under a new Key::URef
     *
     * If a key is returned it is always of [[KeyVariant]].UREF_ID
     */
    static create(value: CLValue): Key | null {
        const valueBytes = value.toBytes();
        let urefBytes = new Uint8Array(UREF_SERIALIZED_LENGTH);
        externals.new_uref(
            urefBytes.dataStart,
            valueBytes.dataStart,
            valueBytes.length
        );
        const urefResult = URef.fromBytes(urefBytes);
        if (urefResult.hasError()) {
            return null;
        }
        return Key.fromURef(urefResult.value);
    }

    /** Deserializes a `Key` from an array of bytes. */
    static fromBytes(bytes: Uint8Array): Result<Key> {
        if (bytes.length < 1) {
            return new Result<Key>(null, BytesreprError.EarlyEndOfStream, 0);
        }
        const tag = bytes[0];
        let currentPos = 1;

        if (tag == KeyVariant.HASH_ID) {
            var hashBytes = bytes.subarray(1, 32 + 1);
            currentPos += 32;

            let key = Key.fromHash(hashBytes);
            let ref = new Ref<Key>(key);
            return new Result<Key>(ref, BytesreprError.Ok, currentPos);
        }
        else if (tag == KeyVariant.UREF_ID) {
            var urefBytes = bytes.subarray(1);
            var urefResult = URef.fromBytes(urefBytes);
            if (urefResult.error != BytesreprError.Ok) {
                return new Result<Key>(null, urefResult.error, 0);
            }
            let key = Key.fromURef(urefResult.value);
            let ref = new Ref<Key>(key);
            return new Result<Key>(ref, BytesreprError.Ok, currentPos + urefResult.position);
        }
        else if (tag == KeyVariant.ACCOUNT_ID) {
            let accountHashBytes = bytes.subarray(1);
            let accountHashResult = AccountHash.fromBytes(accountHashBytes);
            if (accountHashResult.hasError()) {
                return new Result<Key>(null, accountHashResult.error, currentPos);
            }
            currentPos += accountHashResult.position;
            let key = Key.fromAccount(accountHashResult.value);
            let ref = new Ref<Key>(key);
            return new Result<Key>(ref, BytesreprError.Ok, currentPos);
        }
        else {
            return new Result<Key>(null, BytesreprError.FormattingError, currentPos);
        }
    }

    /** Serializes a `Key` into an array of bytes. */
    toBytes(): Array<u8> {
        if(this.variant == KeyVariant.UREF_ID){
            let bytes = new Array<u8>();
            bytes.push(<u8>this.variant)
            bytes = bytes.concat((<URef>this.uref).toBytes());
            return bytes;
        }
        else if (this.variant == KeyVariant.HASH_ID) {
            var hashBytes = <Uint8Array>this.hash;
            let bytes = new Array<u8>(1 + hashBytes.length);
            bytes[0] = <u8>this.variant;
            for (let i = 0; i < hashBytes.length; i++) {
                bytes[i + 1] = hashBytes[i];
            }
            return bytes;
        }
        else if (this.variant == KeyVariant.ACCOUNT_ID) {
            let bytes = new Array<u8>();
            bytes.push(<u8>this.variant);
            bytes = bytes.concat((<AccountHash>this.account).toBytes());
            return bytes;
        }
        unreachable();
        return [];
    }

    /** Checks whether the `Key` is of [[KeyVariant]].UREF_ID. */
    isURef(): bool {
        return this.variant == KeyVariant.UREF_ID;
    }

    /** Converts the `Key` into `URef`. */
    toURef(): URef {
        return <URef>this.uref;
    }

    /** Reads the data stored under this `Key`. */
    read(): Uint8Array | null {
        const keyBytes = this.toBytes();
        let valueSize = new Uint8Array(1);
        const ret = externals.read_value(keyBytes.dataStart, keyBytes.length, valueSize.dataStart);
        const error = Error.fromResult(ret);
        if (error != null) {
            if (error.value() == ErrorCode.ValueNotFound) {
                return null;
            }
            error.revert();
            unreachable();
        }
        // TODO: How can we have `read<T>` that would deserialize host bytes into T?
        return readHostBuffer(valueSize[0]);
    }

    /** Stores a [[CLValue]] under this `Key`. */
    write(value: CLValue): void {
        const keyBytes = this.toBytes();
        const valueBytes = value.toBytes();
        externals.write(
            keyBytes.dataStart,
            keyBytes.length,
            valueBytes.dataStart,
            valueBytes.length
        );
    }

    /** Adds the given `CLValue` to a value already stored under this `Key`. */
    add(value: CLValue): void {
        const keyBytes = this.toBytes();
        const valueBytes = value.toBytes();

        externals.add(
            keyBytes.dataStart,
            keyBytes.length,
            valueBytes.dataStart,
            valueBytes.length
        );
    }

    /** Checks whether two `Key`s are equal. */
    @operator("==")
    equalsTo(other: Key): bool {
        if (this.variant == KeyVariant.UREF_ID) {
            if (other.variant == KeyVariant.UREF_ID) {
                return <URef>this.uref == <URef>other.uref;
            }
            else {
                return false;
            }
        }
        else if (this.variant == KeyVariant.HASH_ID) {
            if (other.variant == KeyVariant.HASH_ID) {
                return checkTypedArrayEqual(<Uint8Array>this.hash, <Uint8Array>other.hash);

            }
            else {
                return false;
            }
        }
        else if (this.variant == KeyVariant.ACCOUNT_ID) {
            if (other.variant == KeyVariant.ACCOUNT_ID) {
                return <AccountHash>this.account == <AccountHash>other.account;
            }
            else {
                return false;
            }
        }
        else {
            return false;
        }
    }

    /** Checks whether two keys are not equal. */
    @operator("!=")
    notEqualsTo(other: Key): bool {
        return !this.equalsTo(other);
    }
}
