import { hex2bin } from "../utils/helpers";
import { U512 } from "../../assembly/bignum";
import { Error } from "../../assembly/bytesrepr";
import { Pair } from "../../assembly/pair";
import { checkArraysEqual } from "../../assembly/utils";
import { arrayToTyped, typedToArray } from "../../assembly/utils";

export function testBigNum512Arith(): bool {
    let a = U512.fromU64(<u64>18446744073709551614); // 2^64-2
    assert(a.toString() == "fffffffffffffffe");

    let b = U512.fromU64(1);

    assert(b.toString() == "1");

    a += b; // a==2^64-1
    assert(a.toString() == "ffffffffffffffff");

    a += b; // a==2^64
    assert(a.toString() == "10000000000000000");

    a -= b; // a==2^64-1
    assert(a.toString() == "ffffffffffffffff");

    a -= b; // a==2^64-2
    assert(a.toString() == "fffffffffffffffe");

    return true;
}

export function testBigNum512Mul(): bool {
    let u64Max = U512.fromU64(<u64>18446744073709551615); // 2^64-1
    assert(u64Max.toString() == "ffffffffffffffff");

    let a = U512.fromU64(<u64>18446744073709551615);

    assert(a.toString() == "ffffffffffffffff");

    a *= u64Max; // (2^64-1) ^ 2
    assert(a.toString() == "fffffffffffffffe0000000000000001");

    a *= u64Max; // (2^64-1) ^ 3
    assert(a.toString(), "fffffffffffffffd0000000000000002ffffffffffffffff");

    a *= u64Max; // (2^64-1) ^ 4
    assert(a.toString() == "fffffffffffffffc0000000000000005fffffffffffffffc0000000000000001");

    a *= u64Max; // (2^64-1) ^ 5
    assert(a.toString() == "fffffffffffffffb0000000000000009fffffffffffffff60000000000000004ffffffffffffffff");

    a *= u64Max; // (2^64-1) ^ 6
    assert(a.toString() == "fffffffffffffffa000000000000000effffffffffffffec000000000000000efffffffffffffffa0000000000000001");
    a *= u64Max; // (2^64-1) ^ 7
    assert(a.toString() == "fffffffffffffff90000000000000014ffffffffffffffdd0000000000000022ffffffffffffffeb0000000000000006ffffffffffffffff");

    a *= u64Max; // (2^64-1) ^ 8
    assert(a.toString() == "fffffffffffffff8000000000000001bffffffffffffffc80000000000000045ffffffffffffffc8000000000000001bfffffffffffffff80000000000000001");

    return true;
}

export function testBigNumZero(): bool {
    let zero = new U512();
    assert(zero.toString() == "0");
    return zero.isZero();
}

export function testBigNonZero(): bool {
    let nonzero = U512.fromU64(<u64>0xffffffff);
    assert(nonzero.toString() == "ffffffff");
    return !nonzero.isZero();
}

export function testBigNumSetHex(): bool {
    let large = U512.fromHex("fffffffffffffff8000000000000001bffffffffffffffc80000000000000045ffffffffffffffc8000000000000001bfffffffffffffff80000000000000001");
    assert(large.toString() == "fffffffffffffff8000000000000001bffffffffffffffc80000000000000045ffffffffffffffc8000000000000001bfffffffffffffff80000000000000001");
    return true;
}

export function testNeg(): bool {
    // big == -big
    // in 2s compliment: big == ~(~big+1)+1
    let big = U512.fromHex("e7ed081ae96850db0c7d5b42094b5e09b0631e6b9f63efe4deb90d7dd677c82f8ce52eccda5b03f5190770a763729ae9ab85c76cd1dc9606ec9dcf2e2528fccb");
    assert(big == -(-big));
    return true;
}

export function testComparison(): bool {
    let zero = new U512();
    assert(zero.isZero());
    let one = U512.fromU64(1);

    let u32Max = U512.fromU64(4294967295);

    assert(zero != one);
    assert(one == one);
    assert(zero == zero);

    assert(zero < one);
    assert(zero <= one);
    assert(one <= one);

    assert(one > zero);
    assert(one >= zero);
    assert(one >= one);

    let large1 = U512.fromHex("a25bd58358ae4cd57ba0a4afcde6e9aa55c801d88854541dfc6ea5e3c1fada9ed9cb1e48b0a2d553faa26e5381743415ae1ec593dc67fc525d18e0b6fdf3f7ae");
    let large2 = U512.fromHex("f254bb1c7f6654f5ad104854709cb5c09009ccd2b78b5364fefd3a5fa99381a173c5498966e77d88d443bd1a650b4bcb8bb8a92013a85a7095330bc79a2e22dc");

    assert(large1.cmp(large2) != 0);
    assert(large1 != large2);
    assert(large2 == large2);
    assert(large1 == large1);

    assert(large1 < large2);
    assert(large1 <= large2);
    assert(large2 <= large2);

    assert(large2 > large1);
    assert(large2 >= large1);
    assert(large2 >= large2);

    assert(large1 > zero);
    assert(large1 > one);
    assert(large1 > u32Max);
    assert(large2 > u32Max);
    assert(large1 >= u32Max);
    assert(u32Max >= one);
    assert(one <= u32Max);
    assert(one != u32Max);
    return true;
}

export function testBits(): bool {
    let zero = new U512();
    assert(zero.bits() == 0);
    let one = U512.fromU64(1);
    assert(one.bits() == 1);

    let value = new U512();
    for (let i = 0; i < 63; i++) {
        value.setU64(1 << i);
        assert(value.bits() == i + 1);
    }

    let shl512P1 = U512.fromHex("10000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
    assert(shl512P1.bits() == 509);

    let u512Max = U512.fromHex("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    assert(u512Max.bits() == 512);

    let mix = U512.fromHex("55555555555");
    assert(mix.bits() == 43);
    return true;
}

export function testDivision(): bool {
    let u512Max = U512.fromHex("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");

    let five = U512.fromU64(5);

    let rand = U512.fromHex("6fdf77a12c44899b8456d394e555ac9b62af0b0e70b79c8f8aa3837116c8c2a5");

    assert(rand.bits() != 0);
    let maybeRes1 = u512Max.divMod(rand);
    assert(maybeRes1 !== null);
    let res1 = <Pair<U512, U512>>(maybeRes1);

    // result

    assert(res1.first.toString(), "249cebb32c9a2f0d1375ddc28138b727428ed6c66f4ca9f0abeb231cff6df7ec7");
    // remainder
    assert(res1.second.toString(), "4d4edcc2e5e0a5416119b88b280018b1b79ffbd0891ae622ee7a6d895e687bbc");
    // recalculate back
    assert((res1.first * rand) + res1.second == u512Max);

    // u512max is multiply of 5
    let divided = u512Max / five;
    let multiplied = divided * five;
    assert(multiplied == u512Max);

    let base10 = "";
    let zero = new U512();
    let ten = U512.fromU64(10);

    assert(five % ten == five);
    assert(ten.divMod(zero) === null);

    while (u512Max > zero) {
        let maybeRes = u512Max.divMod(ten);
        assert(maybeRes !== null);
        let res = <Pair<U512, U512>>(maybeRes);
        base10 = res.second.toString() + base10;
        u512Max = res.first;
    }
    assert(base10 == "13407807929942597099574024998205846127479365820592393377723561443721764030073546976801874298166903427690031858186486050853753882811946569946433649006084095");
    return true;
}

export function testSerializeU512Zero(): bool {
    let truth = hex2bin("00");
    let result = U512.fromBytes(StaticArray.fromArray(truth));
    assert(result.error == Error.Ok);
    assert(result.hasValue());
    let zero = result.value;
    assert(zero.isZero());
    const bytes = zero.toBytes();
    return checkArraysEqual<u8, Array<u8>>(bytes, truth);
};

export function testSerializeU512_3BytesWide(): bool {
    let truth = hex2bin("03807801");
    let result = U512.fromBytes(StaticArray.fromArray(truth));
    assert(result.error == Error.Ok);
    assert(result.hasValue());
    let num = result.value;
    assert(num.toString() == "17880"); // dec: 96384
    const bytes = num.toBytes();
    return checkArraysEqual<u8, Array<u8>>(bytes, truth);
};

export function testSerializeU512_2BytesWide(): bool {
    let truth = hex2bin("020004");
    let result = U512.fromBytes(StaticArray.fromArray(truth));
    assert(result.error == Error.Ok);
    assert(result.hasValue());
    let num = result.value;
    assert(num.toString() == "400"); // dec: 1024
    const bytes = num.toBytes();
    return checkArraysEqual<u8, Array<u8>>(bytes, truth);
};

export function testSerializeU512_1BytesWide(): bool {
    let truth = hex2bin("0101");
    let result = U512.fromBytes(StaticArray.fromArray(truth));
    assert(result.error == Error.Ok);
    assert(result.hasValue());
    let num = result.value;
    assert(num.toString() == "1");
    const bytes = num.toBytes();
    return checkArraysEqual<u8, Array<u8>>(bytes, truth);
};

export function testSerialize100mTimes10(): bool {
    let truth = hex2bin("0400ca9a3b"); // bytesrepr truth

    let hex = "3b9aca00";

    let valU512 = U512.fromHex(hex);
    assert(valU512.toString() == hex);

    let bytes = valU512.toBytes();
    assert(bytes !== null)
    assert(checkArraysEqual<u8, Array<u8>>(bytes, truth));

    let roundTrip = U512.fromBytes(StaticArray.fromArray(bytes));
    assert(roundTrip.error == Error.Ok);
    assert(roundTrip.value.toString() == hex);

    return true;
}

export function testDeserLargeRandomU512(): bool {
    // U512::from_dec_str("11047322357349959198658049652287831689404979606512518998046171549088754115972343255984024380249291159341787585633940860990180834807840096331186000119802997").expect("should create");
    let hex = "d2ee2fd630b02f2ffec88918ba0adaba87e76a294af2e5cc9a4710a23da14a6dde1c73780be2815e979547a949e085aa9279db4c3d1d0fde2361cd2e2d392c75";

    // bytesrepr
    let truth = hex2bin("40752c392d2ecd6123de0f1d3d4cdb7992aa85e049a94795975e81e20b78731cde6d4aa13da210479acce5f24a296ae787bada0aba1889c8fe2f2fb030d62feed2");

    let deser = U512.fromBytes(StaticArray.fromArray(truth));
    assert(deser.error == Error.Ok);
    assert(deser !== null);
    assert(deser.value.toString() == hex);

    let ser = deser.value.toBytes();
    assert(checkArraysEqual<u8, Array<u8>>(ser, truth));

    return true;
}
export function testPrefixOps(): bool {
    let a = U512.fromU64(<u64>18446744073709551615); // 2^64-2
    assert(a.toString() == "ffffffffffffffff");

    let one = U512.fromU64(1);
    assert(one.toString() == "1");

    ++a;
    assert(a.toString() == "10000000000000000");

    ++a;
    assert(a.toString() == "10000000000000001");

    --a;
    assert(a.toString() == "10000000000000000");

    --a;
    assert(a.toString() == "ffffffffffffffff");

    let aCloned = a.clone();

    let aPostInc = a++;
    assert(aPostInc == aCloned);
    assert(a == aCloned + one);

    let aPostDec = a--;
    assert(aPostDec == aCloned);
    assert(a == aCloned - one);
    return true;
}

export function testMinMaxValue(): bool {
    assert(U512.MAX_VALUE.toString() == "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    assert(U512.MIN_VALUE.toString() == "0");
    return true;
}
