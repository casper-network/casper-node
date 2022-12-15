import {typedToArray} from "./utils";
import {Ref} from "./ref";
import {Result, Error as BytesreprError} from "./bytesrepr";

const ED25519_PUBLIC_KEY_LENGTH = 32;
const SECP256K1_PUBLIC_KEY_LENGTH = 33;

export enum PublicKeyVariant {
    /** A public key of Ed25519 type */
    Ed25519 = 1,
    /** A public key of Secp256k1 type */
    Secp256k1 = 2,
}

export class PublicKey {
    constructor(private variant: PublicKeyVariant, private bytes: Array<u8> ) {
    }

    getRawBytes(): Array<u8> {
        return this.bytes;
    }

    getAlgorithmName(): string {
        const ED25519_LOWERCASE: string = "ed25519";
        const SECP256K1_LOWERCASE: string = "secp256k1";

        switch (this.variant) {
            case PublicKeyVariant.Ed25519:
                return ED25519_LOWERCASE;
            case PublicKeyVariant.Secp256k1:
                return SECP256K1_LOWERCASE;
            default:
                return <string>unreachable();
        }
    }

    toBytes(): Array<u8> {
        let variantBytes: Array<u8> = [<u8>this.variant];
        return variantBytes.concat(this.bytes);
    }

    /** Deserializes a `PublicKey` from an array of bytes. */
    static fromBytes(bytes: StaticArray<u8>): Result<PublicKey> {
        if (bytes.length < 1) {
            return new Result<PublicKey>(null, BytesreprError.EarlyEndOfStream, 0);
        }
        const variant = <PublicKeyVariant>bytes[0];
        let currentPos = 1;

        let expectedPublicKeySize: i32;

        switch (variant) {
            case PublicKeyVariant.Ed25519:
                expectedPublicKeySize = ED25519_PUBLIC_KEY_LENGTH;
                break;
            case PublicKeyVariant.Secp256k1:
                expectedPublicKeySize = SECP256K1_PUBLIC_KEY_LENGTH;
                break;
            default:
                return new Result<PublicKey>(null, BytesreprError.FormattingError, 0);
        }
        let publicKeyBytes = bytes.slice(currentPos, currentPos + expectedPublicKeySize);
        currentPos += expectedPublicKeySize;

        let publicKey = new PublicKey(variant, publicKeyBytes);
        let ref = new Ref<PublicKey>(publicKey);
        return new Result<PublicKey>(ref, BytesreprError.Ok, currentPos);
    }
}
