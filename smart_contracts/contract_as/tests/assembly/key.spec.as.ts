import { hex2bin } from "../utils/helpers";
import { checkArraysEqual, checkItemsEqual } from "../../assembly/utils";
import { typedToArray } from "../../assembly/utils";
import { AccountHash } from "../../assembly/key";
import { hex2bin } from "../utils/helpers";

export function testAccountHashEquality(): bool {
    let lhsBytes = hex2bin("00000000000000000000000000000000");
    let rhsBytes = hex2bin("00000000000000000000000000000000");
    const lhs = new AccountHash(lhsBytes);
    const rhs = new AccountHash(rhsBytes);
    return lhs == rhs;
}

export function testAccountHashInequality(): bool {
    let lhsBytes = hex2bin("00000000000000000000000000000000");
    let rhsBytes = hex2bin("00000000000000000000000000000001");
    const lhs = new AccountHash(lhsBytes);
    const rhs = new AccountHash(rhsBytes);
    return lhs != rhs;
}

export function testAccountHashGreaterThan(): bool {
    let lhsBytes = hex2bin("00000000000000000000000000000001");
    let rhsBytes = hex2bin("00000000000000000000000000000000");
    const lhs = new AccountHash(lhsBytes);
    const rhs = new AccountHash(rhsBytes);
    return lhs > rhs;
}

export function testAccountHashLowerThan(): bool {
    let lhsBytes = hex2bin("00000000000000000000000000000000");
    let rhsBytes = hex2bin("00000000000000000000000000000001");
    const lhs = new AccountHash(lhsBytes);
    const rhs = new AccountHash(rhsBytes);
    return lhs < rhs;
}
