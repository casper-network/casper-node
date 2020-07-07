/**
 * Length of [[URef]] address field.
 * @internal
 */
export const UREF_ADDR_LENGTH = 32;

/**
 * Length of hash variant of a [[Key]].
 * @internal
 */
export const KEY_HASH_LENGTH = 32;

/**
 * Serialized length of [[AccessRights]] field.
 * @internal
 */
export const ACCESS_RIGHTS_SERIALIZED_LENGTH = 1;

/**
 * Serialized length of [[URef]] object.
 * @internal
 */
export const UREF_SERIALIZED_LENGTH = UREF_ADDR_LENGTH + ACCESS_RIGHTS_SERIALIZED_LENGTH;

/**
 * Serialized length of ID of key.
 * @internal
 */
export const KEY_ID_SERIALIZED_LENGTH: i32 = 1; // u8 used to determine the ID

/**
 * Serialized length of [[Key]] object.
 */
export const KEY_UREF_SERIALIZED_LENGTH = KEY_ID_SERIALIZED_LENGTH + UREF_SERIALIZED_LENGTH;