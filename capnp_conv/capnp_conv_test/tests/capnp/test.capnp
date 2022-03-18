@0xc90daeac68e62b2a;

struct TestStructInner {
        innerU8 @0: UInt8;
}

struct TestUnion {
    union {
        one @0: UInt64;
        two @1: TestStructInner;
        three @2: Void;
        four @3: Text;
    }
}

struct ListUnion {
    union {
        empty @0: Void;
        withList @1: List(TestStructInner);
        withData @2: Data;
        testUnion @3: TestUnion;
        inlineInnerUnion: union {
                ab @4: UInt32;
                cd @5: UInt64;
        }
    }
}

struct TestStruct {
    myBool @0: Bool;
    myInt8 @1: Int8;
    myInt16 @2: Int16;
    myInt32 @3: Int32;
    myInt64 @4: Int64;
    myUint8 @5: UInt8;
    myUint16 @6: UInt16;
    myUint32 @7: UInt32;
    myUint64 @8: UInt64;
    # my_float32: f32,
    # my_float64: f64,
    myText @9: Text;
    myData @10: Data;
    structInner @11: TestStructInner;
    myPrimitiveList @12: List(UInt16);
    myList @13: List(TestStructInner);
    inlineUnion: union {
            first @14: UInt64;
            second @15: TestStructInner;
            third @16: Void;
    }
    externalUnion @17: TestUnion;
    listUnion @18: ListUnion;
}

struct FloatStruct {
    myFloat32 @0: Float32;
    myFloat64 @1: Float64;
}

struct GenericStruct {
    a @0: UInt32;
    b @1: UInt64;
    c @2: UInt8;
    d @3: Data;
    e @4: List(TestStructInner);
    f @5: TestStructInner;
}

struct GenericEnum {
    union {
        a @0: UInt32;
        b @1: TestStructInner;
        c @2: UInt64;
        d @3: Data;
    }
}

struct InnerGeneric {
    a @0: UInt32;
}

struct ListGeneric {
    list @0: List(InnerGeneric);
}

# A custom made 128 bit data structure.
struct Buffer128 {
        x0 @0: UInt64;
        x1 @1: UInt64;
}

# Unsigned 128 bit integer
struct CustomUInt128 {
        inner @0: Buffer128;
}


struct TestWithStruct {
        a @0: CustomUInt128;
        b @1: UInt64;
}

struct TestWithEnum {
    union {
        a @0: CustomUInt128;
        b @1: UInt64;
        c @2: Void;
    }
}

struct Map(Key, Value) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :Value;
  }
}