/**
 * Encodes an UTF8 string into bytes.
 * @param str Input string.
 */
export function encodeUTF8(str: String): Uint8Array {
  let utf8Bytes = String.UTF8.encode(str);
  return Uint8Array.wrap(utf8Bytes);
}

/** Converts typed array to array */
export function typedToArray(arr: Uint8Array): Array<u8> {
  let result = new Array<u8>(arr.length);
  for (let i = 0; i < arr.length; i++) {
      result[i] = arr[i];
  }
  return result;
}

/** Converts array to typed array */
export function arrayToTyped(arr: Array<u8>): Uint8Array {
  let result = new Uint8Array(arr.length);
  for (let i = 0; i < arr.length; i++) {
      result[i] = arr[i];
  }
  return result;
}

/** Checks if items in two unordered arrays are equal */
export function checkItemsEqual<T>(a: Array<T>, b: Array<T>): bool {
  for (let i = 0; i < a.length; i++) {
    const idx = b.indexOf(a[i]);
    if (idx == -1) {
      return false;
    }
    b.splice(idx, 1);
  }
  return b.length === 0;
}

/** Checks if two ordered arrays are equal */
export function checkArraysEqual<T>(a: Array<T>, b: Array<T>, len: i32 = 0): bool {
  if (!len) {
    len = a.length;
    if (len != b.length) return false;
    if (a === b) return true;
  }
  for (let i = 0; i < len; i++) {
    if (isFloat<T>()) {
      if (isNaN(a[i]) && isNaN(b[i])) continue;
    }
    if (a[i] != b[i]) return false;
  }
  return true;
}


/** Checks if two ordered arrays are equal */
export function checkTypedArrayEqual(a: Uint8Array, b: Uint8Array, len: i32 = 0): bool {
  if (!len) {
    len = a.length;
    if (len != b.length) return false;
    if (a === b) return true;
  }
  for (let i = 0; i < len; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}
