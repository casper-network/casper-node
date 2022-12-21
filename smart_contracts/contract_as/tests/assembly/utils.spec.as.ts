import { hex2bin } from "../utils/helpers";
import { checkArraysEqual, checkItemsEqual } from "../../assembly/utils";
import { typedToArray } from "../../assembly/utils";
import { Pair } from "../../assembly/pair";

export function testHex2Bin(): bool {
    let lhs = hex2bin("deadbeef");
    let rhs: Array<u8> = [222, 173, 190, 239];
    return checkArraysEqual<u8>(lhs, rhs);
}

export function testcheckArraysEqual(): bool {
    assert(checkArraysEqual<u8, Array<u8>>(<u8[]>[], <u8[]>[]));
    assert(!checkArraysEqual<u8, Array<u8>>(<u8[]>[1, 2, 3], <u8[]>[1]));
    return checkArraysEqual<u8, Array<u8>>(<u8[]>[1, 2, 3], <u8[]>[1, 2, 3]);
}

export function testItemsEqual(): bool {
    let lhs: u32[] = [6,3,5,2,4,1];
    let rhs: u32[] = [1,2,3,4,5,6];
    return checkItemsEqual(lhs, rhs);
}

export function testItemsNotEqual(): bool {
    let lhs1: u32[] = [1,2,3,4,5];
    let rhs1: u32[] = [1,2,3,4,5,6];

    let lhs2: u32[] = [1,2,3,4,5,6];
    let rhs2: u32[] = [1,2,3,4,5];
    assert(!checkItemsEqual(lhs1, rhs1));
    assert(!checkItemsEqual(rhs2, lhs2));
    return true;
}

export function testItemsNotEqual2(): bool {
    let lhs: u32[] = [1,2,3];
    let rhs: u32[] = [1,3,1];
    assert(!checkItemsEqual(lhs, rhs));
    assert(!checkItemsEqual(rhs, lhs));
    return true;
}

function comp(lhs: Pair<String, String>, rhs: Pair<String, String>): bool {
    return lhs.equalsTo(rhs);
}

export function testPairItemsEqual(): bool {
    let lhs: Pair<String, String>[] = [
        new Pair<String, String>("Key1", "Value1"),
        new Pair<String, String>("Key2", "Value2"),
        new Pair<String, String>("Key3", "Value3"),
    ];
    let rhs: Pair<String, String>[] = [
        new Pair<String, String>("Key2", "Value2"),
        new Pair<String, String>("Key3", "Value3"),
        new Pair<String, String>("Key1", "Value1"),
    ];
    return checkItemsEqual(lhs, rhs);
}

export function testEmptyItemsEqual(): bool {
    let lhs: Pair<String, String>[] = [
    ];
    let rhs: Pair<String, String>[] = [
    ];
    return checkItemsEqual(lhs, rhs);
}
