//@ts-nocheck
import { Error } from "../../../../contract_as/assembly/error";
import { Key } from "../../../../contract_as/assembly/key";
import { newDictionary, dictionaryGet, dictionaryPut, dictionaryRead } from "../../../../contract_as/assembly/dictionary";
import { CLValue } from "../../../../contract_as/assembly/clvalue";
import { checkArraysEqual } from "../../../../contract_as/assembly/utils";

const DICTIONARY_NAME = "dictionary-name";
const DICTIONARY_ITEM_KEY = "dictionary-item-key";
const DICTIONARY_VALUE = "dictionary-value";

export function call(): void {
    let dictionary_seed_uref = newDictionary(DICTIONARY_NAME);
    let dictionary_cl_value = CLValue.fromString(DICTIONARY_VALUE);
    dictionaryPut(dictionary_seed_uref, DICTIONARY_ITEM_KEY, dictionary_cl_value);
    let dictionary_key = Key.createDictionaryKey(dictionary_seed_uref, DICTIONARY_ITEM_KEY);
    let valueViaDictionaryRead = dictionaryRead(dictionary_key);
    if (!valueViaDictionaryRead) {
        Error.fromUserError(14).revert()
        return
    }
    let valueViaDictionaryGet = dictionaryGet(dictionary_seed_uref, DICTIONARY_ITEM_KEY);
    if (!valueViaDictionaryGet) {
        Error.fromUserError(15).revert()
        return
    }
    if (!checkArraysEqual<u8, StaticArray<u8>>(valueViaDictionaryRead, valueViaDictionaryGet)) {
        Error.fromUserError(18).revert()
        return
    }
}
