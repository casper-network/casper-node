import {
    fromBytesU64, toBytesU64,
    fromBytesStringList, toBytesStringList,
    fromBytesU32, toBytesU32,
    fromBytesU8, toBytesU8,
    toBytesMap, fromBytesMap,
    toBytesPair,
    toBytesString, fromBytesString,
    toBytesVecT,
    Error
} from "../../assembly/bytesrepr";
import { CLValue, CLType, CLTypeTag } from "../../assembly/clvalue";
import { Key, KeyVariant, AccountHash } from "../../assembly/key";
import { URef, AccessRights } from "../../assembly/uref";
import { Option } from "../../assembly/option";
import { hex2bin } from "../utils/helpers";
import { checkArraysEqual, checkItemsEqual } from "../../assembly/utils";
import { typedToArray, }from "../../assembly/utils";
import { Pair } from "../../assembly/pair";
import { EntryPointAccess, PublicAccess, GroupAccess, EntryPoint, EntryPoints, EntryPointType } from "../../assembly";

// adding the prefix xtest to one of these functions will cause the test to
// be ignored via the defineTestsFromModule function in spec.tsgit

export function testDeserializeInvalidU8(): bool {
    const bytes: u8[] = [];
    let deser = fromBytesU8(StaticArray.fromArray(bytes));
    assert(deser.error == Error.EarlyEndOfStream);
    assert(deser.position == 0);
    return !deser.hasValue();
}

export function testDeSerU8(): bool {
    const truth: u8[] = [222];
    let ser = toBytesU8(222);
    assert(checkArraysEqual<u8, Array<u8>>(ser, truth));
    let deser = fromBytesU8(StaticArray.fromArray(ser));
    assert(deser.error == Error.Ok);
    return deser.value == 222;
}

export function xtestDeSerU8_Zero(): bool {
    // Used for deserializing Weight (for example)
    // NOTE: Currently probably unable to check if `foo(): U8 | null` result is null
    const truth: u8[] = [0];
    let ser = toBytesU8(0);
    assert(checkArraysEqual<u8, Array<u8>>(ser, truth));
    let deser = fromBytesU8(StaticArray.fromArray(ser));
    assert(deser.error == Error.Ok);
    return deser.value == 0;
}

export function testDeSerU32(): bool {
    const truth: u8[] = [239, 190, 173, 222];
    let ser = toBytesU32(3735928559);
    assert(checkArraysEqual<u8, Array<u8>>(ser, truth));
    let deser = fromBytesU32(StaticArray.fromArray(ser));
    assert(deser.error == Error.Ok);
    assert(deser.position == 4);
    return deser.value == 0xdeadbeef;
}

export function testDeSerZeroU32(): bool {
    const truth: u8[] = [0, 0, 0, 0];
    let ser = toBytesU32(0);
    assert(checkArraysEqual<u8, Array<u8>>(ser, truth));
    let deser = fromBytesU32(StaticArray.fromArray(ser));
    assert(deser.error == Error.Ok);
    assert(deser.hasValue());
    return deser.value == 0;
}

export function testDeserializeU64_1024(): bool {
    const truth = hex2bin("0004000000000000");
    var deser = fromBytesU64(StaticArray.fromArray(truth));
    assert(deser.error == Error.Ok);
    assert(deser.position == 8);
    return deser.value == <u64>1024;
}

export function testDeserializeU64_zero(): bool {
    const truth = hex2bin("0000000000000000");
    var deser = fromBytesU64(StaticArray.fromArray(truth));
    assert(deser.error == Error.Ok);
    assert(deser.position == 8);
    assert(deser.hasValue());
    return deser.value == 0;
}

export function testDeserializeU64_u32max(): bool {
    const truth = hex2bin("ffffffff00000000");
    const deser = fromBytesU64(StaticArray.fromArray(truth));
    assert(deser.error == Error.Ok);
    assert(deser.position == 8);
    return deser.value == 0xffffffff;
}

export function testDeserializeU64_u32max_plus1(): bool {
    const truth = hex2bin("0000000001000000");
    const deser = fromBytesU64(StaticArray.fromArray(truth));
    assert(deser.hasValue());
    assert(deser.error == Error.Ok);
    assert(deser.position == 8);
    return deser.value == 4294967296;
}

export function testDeserializeU64_EOF(): bool {
    const truth = hex2bin("0000");
    const deser = fromBytesU64(StaticArray.fromArray(truth));
    assert(deser.error == Error.EarlyEndOfStream);
    assert(deser.position == 0);
    return !deser.hasValue();
}

export function testDeserializeU64_u64max(): bool {
    const truth = hex2bin("feffffffffffffff");
    const deser = fromBytesU64(StaticArray.fromArray(truth));
    assert(deser.error == Error.Ok);
    assert(deser.position == 8);
    return deser.value == <u64>18446744073709551614;
}

export function testDeSerListOfStrings(): bool {
    const truth = hex2bin("03000000030000006162630a0000003132333435363738393006000000717765727479");
    const result = fromBytesStringList(StaticArray.fromArray(truth));
    assert(result.error == Error.Ok);
    assert(result.hasValue());
    const strList = result.value;
    assert(result.position == truth.length);

    assert(checkArraysEqual<String, Array<String>>(strList, <String[]>[
        "abc",
        "1234567890",
        "qwerty",
    ]));

    let lhs = toBytesStringList(strList);
    let rhs = truth;
    return checkArraysEqual<u8, Array<u8>>(lhs, rhs);
};

export function testDeSerEmptyListOfStrings(): bool {
    const truth = hex2bin("00000000");
    const result = fromBytesStringList(StaticArray.fromArray(truth));
    assert(result.error == Error.Ok);
    assert(result.position == 4);
    return checkArraysEqual<String, Array<String>>(<String[]>result.value, <String[]>[]);
};

export function testDeSerEmptyMap(): bool {
    const truth = hex2bin("00000000");
    const result = fromBytesMap<String, Key>(
        StaticArray.fromArray(truth),
        fromBytesString,
        Key.fromBytes);
    assert(result.error == Error.Ok);
    assert(result.hasValue());
    assert(result.position == 4);
    return checkArraysEqual<Pair<String, Key>, Array<Pair<String, Key>>>(result.value, <Array<Pair<String, Key>>>[]);
};

export function testSerializeMap(): bool {
    // let mut m = BTreeMap::new();
    // m.insert("Key1".to_string(), "Value1".to_string());
    // m.insert("Key2".to_string(), "Value2".to_string());
    // let truth = m.to_bytes().unwrap();
    const truth = hex2bin(
        "02000000040000004b6579310600000056616c756531040000004b6579320600000056616c756532"
    );
    const pairs = new Array<Pair<String, String>>();
    pairs.push(new Pair("Key1", "Value1"));
    pairs.push(new Pair("Key2", "Value2"));
    const serialized = toBytesMap(pairs, toBytesString, toBytesString);
    assert(checkArraysEqual<u8, Array<u8>>(serialized, truth));

    const deser = fromBytesMap<String, String>(
        StaticArray.fromArray(serialized),
        fromBytesString,
        fromBytesString);

    assert(deser.error == Error.Ok);
    assert(deser.position == truth.length);
    let listOfPairs = deser.value;

    let res1 = false;
    let res2 = false;
    for (let i = 0; i < listOfPairs.length; i++) {
        if (listOfPairs[i].first == "Key1" && listOfPairs[i].second == "Value1") {
            res1 = true;
        }
        if (listOfPairs[i].first == "Key2" && listOfPairs[i].second == "Value2") {
            res2 = true;
        }
    }
    assert(res1);
    assert(res2);
    return listOfPairs.length == 2;
}

export function testToBytesVecT(): bool {
    // let args = ("get_payment_purse",).parse().unwrap().to_bytes().unwrap();
    const truth = hex2bin("0100000015000000110000006765745f7061796d656e745f70757273650a");
    let serialize = function (item: CLValue): Array<u8> { return item.toBytes(); };
    let serialized = toBytesVecT<CLValue>([
        CLValue.fromString("get_payment_purse"),
    ], serialize);
    return checkArraysEqual<u8, Array<u8>>(serialized, truth);
}

export function testKeyOfURefVariantSerializes(): bool {
    // URef with access rights
    const truth = hex2bin("022a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a07");
    const urefBytes = hex2bin("2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a");
    let uref = new URef(StaticArray.fromArray(urefBytes), AccessRights.READ_ADD_WRITE);
    let key = Key.fromURef(uref);
    let serialized = key.toBytes();

    return checkArraysEqual<u8, Array<u8>>(serialized, truth);
};

export function testDeSerString(): bool {
    // Rust: let bytes = "hello_world".to_bytes().unwrap();
    const truth = hex2bin("0b00000068656c6c6f5f776f726c64");

    const ser = toBytesString("hello_world");
    assert(checkArraysEqual<u8, Array<u8>>(ser, truth));

    const deser = fromBytesString(StaticArray.fromArray(ser));
    assert(deser.error == Error.Ok);
    return deser.value == "hello_world";
}

export function testDeSerIncompleteString(): bool {
    // Rust: let bytes = "hello_world".to_bytes().unwrap();
    const truth = hex2bin("0b00000068656c6c6f5f776f726c");
    // last byte removed from the truth to signalize incomplete data
    const deser = fromBytesString(StaticArray.fromArray(truth));
    assert(deser.error == Error.EarlyEndOfStream);
    return !deser.hasValue();
}

export function testDecodeURefFromBytesWithoutAccessRights(): bool {
    const truth = hex2bin("2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a00");
    let urefResult = URef.fromBytes(StaticArray.fromArray(truth));
    assert(urefResult.error == Error.Ok);
    assert(urefResult.hasValue());
    let uref = urefResult.value;

    let urefBytes = new Array<u8>(32);
    urefBytes.fill(42);


    assert(checkArraysEqual<u8, Array<u8>>(uref.getBytes().slice(0), urefBytes));
    assert(uref.getAccessRights() === AccessRights.NONE);
    let serialized = uref.toBytes();
    return checkArraysEqual<u8, Array<u8>>(serialized, truth);
}

export function testDecodeURefFromBytesWithAccessRights(): bool {
    const truth = hex2bin("2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a07");
    const urefResult = URef.fromBytes(StaticArray.fromArray(truth));
    assert(urefResult.error == Error.Ok);
    assert(urefResult.position == truth.length);
    const uref = urefResult.value;
    assert(checkArraysEqual<u8, Array<u8>>(uref.getBytes().slice(0), <u8[]>[
        42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
        42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
        42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
        42, 42,
    ]));
    return uref.getAccessRights() == 0x07; // NOTE: 0x07 is READ_ADD_WRITE
}

export function testDecodedOptionalIsNone(): bool {
    let optionalSome = new StaticArray<u8>(10);
    optionalSome[0] = 0;
    let res = Option.fromBytes(optionalSome);
    assert(res.isNone(), "option should be NONE");
    return !res.isSome();
};

export function testDecodedOptionalIsSome(): bool {
    let optionalSome = new StaticArray<u8>(10);
    for (let i = 0; i < 10; i++) {
        optionalSome[i] = <u8>i + 1;
    }
    let res = Option.fromBytes(optionalSome);
    assert(res !== null);
    let unwrapped = res.unwrap();
    assert(unwrapped !== null, "unwrapped should not be null");
    let values = <Array<u8>>unwrapped;
    let rhs: Array<u8> = [2, 3, 4, 5, 6, 7, 8, 9, 10];
    return checkArraysEqual<u8, Array<u8>>(values, rhs);
};

export function testDeserMapOfNamedKeys(): bool {

    let extraBytes = "fffefd";
    let truthBytes = "0300000001000000410001010101010101010101010101010101010101010101010101010101010101010200000042420202020202020202020202020202020202020202020202020202020202020202020703000000434343010303030303030303030303030303030303030303030303030303030303030303";

    let truth = hex2bin(truthBytes + extraBytes);

    const mapResult = fromBytesMap<String, Key>(
        StaticArray.fromArray(truth),
        fromBytesString,
        Key.fromBytes);
    assert(mapResult.error == Error.Ok);
    let deserializedBytes = mapResult.position;
    assert(<u32>deserializedBytes == <i32>truth.length - hex2bin(extraBytes).length);

    let deser = mapResult.value;
    assert(deser.length === 3);

    assert(deser[0].first == "A");
    assert(deser[0].second.variant == KeyVariant.ACCOUNT_ID);

    let accountBytes = new Array<u8>(32);
    accountBytes.fill(1);

    assert(checkArraysEqual<u8, Array<u8>>((<AccountHash>deser[0].second.account).bytes, accountBytes));
    assert(checkArraysEqual<u8, Array<u8>>((<AccountHash>deser[0].second.account).bytes, accountBytes));

    //

    assert(deser[1].first == "BB");
    assert(deser[1].second.variant == KeyVariant.UREF_ID);

    let urefBytes = new Array<u8>(32);
    urefBytes.fill(2);

    assert(deser[1].second.uref !== null);
    let deser1Uref = <URef>deser[1].second.uref;
    assert(checkArraysEqual<u8, Array<u8>>(<Array<u8>>deser1Uref.bytes.slice(0), urefBytes));
    assert(deser1Uref.accessRights == AccessRights.READ_ADD_WRITE);

    //

    assert(deser[2].first == "CCC");
    assert(deser[2].second.variant == KeyVariant.HASH_ID);

    let hashBytes = new Array<u8>(32);
    hashBytes.fill(3);

    assert(checkArraysEqual<u8, Array<u8>>((<StaticArray<u8>>deser[2].second.hash).slice(0), hashBytes));

    // Compares to truth

    let truthObj = new Array<Pair<String, Key>>();
    let keyA = Key.fromAccount(new AccountHash(accountBytes));
    truthObj.push(new Pair<String, Key>("A", keyA));

    let urefB = new URef(StaticArray.fromArray(urefBytes), AccessRights.READ_ADD_WRITE);
    let keyB = Key.fromURef(urefB);
    truthObj.push(new Pair<String, Key>("BB", keyB));

    let keyC = Key.fromHash(StaticArray.fromArray(hashBytes));
    truthObj.push(new Pair<String, Key>("CCC", keyC));

    assert(truthObj.length === deser.length);
    assert(truthObj[0] == deser[0]);
    assert(truthObj[1] == deser[1]);
    assert(truthObj[2] == deser[2]);

    assert(checkArraysEqual<Pair<String, Key>, Array<Pair<String, Key>>>(truthObj, deser));
    assert(checkItemsEqual(truthObj, deser));

    return true;
}

function useEntryPointAccess(entryPointAccess: EntryPointAccess): Array<u8> {
    return entryPointAccess.toBytes();
}

export function testPublicEntryPointAccess(): bool {
    let publicTruth = hex2bin("01");
    let publicAccess = new PublicAccess();
    let bytes = useEntryPointAccess(publicAccess);
    assert(bytes.length == 1);
    assert(checkArraysEqual<u8, Array<u8>>(publicTruth, bytes));
    return true;
}

export function testGroupEntryPointAccess(): bool {
    let publicTruth = hex2bin("02030000000700000047726f757020310700000047726f757020320700000047726f75702033");
    let publicAccess = new GroupAccess(["Group 1", "Group 2", "Group 3"]);
    let bytes = useEntryPointAccess(publicAccess);
    assert(checkArraysEqual<u8, Array<u8>>(publicTruth, bytes));
    return true;
}

export function testComplexCLType(): bool {
    let type = CLType.byteArray(32);
    let bytes = type.toBytes();
    let truth = hex2bin("0f20000000");
    assert(checkArraysEqual<u8, Array<u8>>(truth, bytes));

    return true;
}

export function testToBytesEntryPoint(): bool {
    let entryPoints = new EntryPoints();
    let args = new Array<Pair<String, CLType>>();
    args.push(new Pair("param1", new CLType(CLTypeTag.U512)));
    let entryPoint = new EntryPoint("delegate", args, new CLType(CLTypeTag.Unit), new PublicAccess(), EntryPointType.Contract);
    entryPoints.addEntryPoint(entryPoint);
    let bytes = entryPoints.toBytes();
    let truth = hex2bin("010000000800000064656c65676174650800000064656c65676174650100000006000000706172616d3108090101");
    assert(checkArraysEqual<u8, Array<u8>>(truth, bytes));
    return true;
}
