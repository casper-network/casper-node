use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution::ExecError};
use casper_types::{
    bytesrepr::Bytes, runtime_args, ApiError, Gas, HostFunctionCostsV1, OpcodeCosts, RuntimeArgs,
    WasmConfig, WasmV1Config, U256,
};
use num_traits::Zero;

const CONTRACT: &str = "alt_bn128.wasm";
const OPERATIONS: [(&str, usize); 3] = [("add", 128), ("mul", 96), ("pairing", 192)];

type Coordinate = [u8; 32];

fn coordinates(values: &[&str]) -> Vec<Coordinate> {
    values
        .iter()
        .map(|value| {
            let value = U256::from_str_radix(value, 16).unwrap();
            let mut bytes = [0; 32];
            value.to_little_endian(&mut bytes);
            bytes
        })
        .collect()
}

fn bytes(values: &[Coordinate]) -> Vec<u8> {
    values.iter().flatten().copied().collect()
}

fn initialize() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());
    builder
}

fn initialize_with_curve_costs() -> LmdbWasmTestBuilder {
    let mut builder = initialize();
    let wasm = builder.chainspec().wasm_config;
    let defaults = HostFunctionCostsV1::default();
    let costs = HostFunctionCostsV1 {
        alt_bn128_add: defaults.alt_bn128_add,
        alt_bn128_mul: defaults.alt_bn128_mul,
        alt_bn128_pairing: defaults.alt_bn128_pairing,
        ..HostFunctionCostsV1::zero()
    };
    let v1 = WasmV1Config::new(
        wasm.v1().max_memory(),
        wasm.v1().max_stack_height(),
        OpcodeCosts::zero(),
        costs,
    );
    let chainspec = builder
        .chainspec()
        .clone()
        .with_wasm_config(WasmConfig::new(wasm.messages_limits(), v1, *wasm.v2()));
    builder.with_chainspec(chainspec);
    builder
}

fn check_api(
    builder: &mut LmdbWasmTestBuilder,
    operation: u8,
    input: Vec<Coordinate>,
    expected: Vec<u8>,
    expected_error: Option<ApiError>,
) {
    let request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT,
        runtime_args! {
            "operation" => operation,
            "input" => input,
            "expected" => Bytes::from(expected),
            "expected_error" => expected_error.map(u32::from),
        },
    )
    .build();
    builder.exec(request).expect_success();
}

fn add_vector() -> (Vec<Coordinate>, Vec<u8>) {
    (
        coordinates(&[
            "18b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9",
            "063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f37266",
            "07c2b7f58a84bd6145f00c9c2bc0bb1a187f20ff2c92963a88019e7c6a014eed",
            "06614e20c147e940f2d70da3f74c9a17df361706a4485c742bd6788478fa17d7",
        ]),
        bytes(&coordinates(&[
            "2243525c5efd4b9c3d3c45ac0ca3fe4dd85e830a4ce6b65fa1eeaee202839703",
            "301d1d33be6da8e509df21cc35964723180eed7532537db9ae5e7d48f195c915",
        ])),
    )
}

fn mul_vector() -> (Vec<Coordinate>, Vec<u8>) {
    (
        coordinates(&[
            "2bd3e6d0f3b142924f5ca7b49ce5b9d54c4703d7ae5648e61d02268b1a0a9fb7",
            "21611ce0a6af85915e2f1d70300909ce2e49dfad4a4619c8390cae66cefdb204",
            "00000000000000000000000000000000000000000000000011138ce750fa15c2",
        ]),
        bytes(&coordinates(&[
            "070a8d6a982153cae4be29d434e8faef8a47b274a053f5a4ee2a6c9c13c31e5c",
            "031b8ce914eba3a9ffb989f9cdd5b0f01943074bf4f0f315690ec3cec6981afc",
        ])),
    )
}

fn pairing_vector() -> Vec<Coordinate> {
    coordinates(&[
        "1c76476f4def4bb94541d57ebba1193381ffa7aa76ada664dd31c16024c43f59",
        "3034dd2920f673e204fee2811c678745fc819b55d3e9d294e45c9b03a76aef41",
        "04bf11ca01483bfa8b34b43561848d28905960114c8ac04049af4b6315a41678",
        "209dd15ebff5d46c4bd888e51a93cf99a7329636c63514396b4a452003a35bf7",
        "120a2a4cf30c1bf9845f20c6fe39e07ea2cce61f0c9bb048165fe5e4de877550",
        "2bb8324af6cfc93537a2ad1a445cfd0ca2a71acd7ac41fadbf933c2a51be344d",
        "111e129f1cf1097710d41c4ac70fcdfa5ba2023c6ff1cbeac322de49d1b6df7c",
        "2032c61a830e3c17286de9462bf242fca2883585b93870a73853face6a6bf411",
        "1800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed",
        "198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c2",
        "12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
        "090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b",
    ])
}

#[ignore]
#[test]
fn should_match_vm2_reference_vectors() {
    let mut builder = initialize();
    let (input, expected) = add_vector();
    check_api(&mut builder, 0, input, expected, None);
    let (input, expected) = mul_vector();
    check_api(&mut builder, 1, input, expected, None);
    let pairs = pairing_vector();
    check_api(&mut builder, 2, pairs.clone(), vec![1], None);
    check_api(&mut builder, 2, pairs[..6].to_vec(), vec![0], None);
    check_api(&mut builder, 2, vec![], vec![1], None);
}

#[ignore]
#[test]
fn should_handle_infinity_and_reduce_scalars() {
    let mut builder = initialize();
    let generator = bytes(&coordinates(&["1", "2"]));
    check_api(
        &mut builder,
        0,
        coordinates(&["1", "2", "0", "0"]),
        generator.clone(),
        None,
    );
    check_api(&mut builder, 0, vec![[0; 32]; 4], vec![0; 64], None);
    check_api(
        &mut builder,
        1,
        coordinates(&["1", "2", "0"]),
        vec![0; 64],
        None,
    );
    check_api(
        &mut builder,
        1,
        coordinates(&["0", "0", "2"]),
        vec![0; 64],
        None,
    );
    check_api(&mut builder, 2, vec![[0; 32]; 6], vec![1], None);
    let order = "30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001";
    check_api(
        &mut builder,
        1,
        coordinates(&["1", "2", order]),
        vec![0; 64],
        None,
    );
    let order_plus_one = "30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000002";
    check_api(
        &mut builder,
        1,
        coordinates(&["1", "2", order_plus_one]),
        generator,
        None,
    );
}

#[ignore]
#[test]
fn should_return_api_errors_for_invalid_coordinates_and_points() {
    let mut builder = initialize();
    for (operation, count) in [(0, 4), (1, 3), (2, 6)] {
        // Multiplication's final input is a scalar, which accepts every 256-bit integer.
        let coordinate_count = if operation == 1 { 2 } else { count };
        for index in 0..coordinate_count {
            let mut input = vec![[0; 32]; count];
            input[index] = [0xff; 32];
            check_api(
                &mut builder,
                operation,
                input,
                vec![],
                Some(ApiError::InvalidArgument),
            );
        }
        let mut input = vec![[0; 32]; count];
        input[0][0] = 1;
        input[1][0] = 1;
        check_api(
            &mut builder,
            operation,
            input,
            vec![],
            Some(ApiError::InvalidArgument),
        );
    }
    let mut invalid_g2 = vec![[0; 32]; 6];
    invalid_g2[2][0] = 1;
    invalid_g2[4][0] = 1;
    check_api(
        &mut builder,
        2,
        invalid_g2,
        vec![],
        Some(ApiError::InvalidArgument),
    );
}

fn raw_wasm(
    operation: &str,
    input: &[u8],
    input_ptr: u32,
    input_size: u32,
    output_ptr: u32,
    status: i32,
    checks: &str,
) -> Vec<u8> {
    let input: String = input.iter().map(|byte| format!("\\{byte:02x}")).collect();
    let sentinel = "\\a5".repeat(65);
    wat::parse_str(format!(
        r#"(module
        (import "env" "casper_alt_bn128_{operation}" (func $op (param i32 i32 i32) (result i32)))
        (memory 1 1)
        (data (i32.const 0) "{input}")
        (data (i32.const 1024) "{sentinel}")
        (func (export "call")
            (if (i32.ne
                (call $op (i32.const {input_ptr}) (i32.const {input_size}) (i32.const {output_ptr}))
                (i32.const {status}))
                (then unreachable))
            {checks}
        ))"#
    ))
    .unwrap()
}

fn check_output(output_ptr: u32, expected: &[u8]) -> String {
    expected.iter().enumerate().map(|(offset, byte)| {
        let address = output_ptr + offset as u32;
        format!("(if (i32.ne (i32.load8_u (i32.const {address})) (i32.const {byte})) (then unreachable))")
    }).collect()
}

fn execute_raw(builder: &mut LmdbWasmTestBuilder, wasm: Vec<u8>, limit: Option<u64>) {
    let mut request =
        ExecuteRequestBuilder::module_bytes(*DEFAULT_ACCOUNT_ADDR, wasm, RuntimeArgs::new())
            .build();
    if let Some(limit) = limit {
        request.session.gas_limit = Gas::new(limit);
    }
    builder.exec(request);
}

#[ignore]
#[test]
fn should_reject_malformed_raw_lengths_without_writing_output() {
    let mut builder = initialize();
    let checks = check_output(1024, &[0xa5; 64]);
    for (operation, size) in OPERATIONS {
        let mut sizes = vec![1, size - 1, size + 1];
        if operation != "pairing" {
            sizes.push(0);
            sizes.push(2 * size);
        } else {
            // A bytesrepr-style count prefix is not part of the ABI.
            sizes.push(size + 4);
        }
        for size in sizes {
            let wasm = raw_wasm(
                operation,
                &vec![0; size],
                0,
                size as u32,
                1024,
                u32::from(ApiError::InvalidArgument) as i32,
                &checks,
            );
            execute_raw(&mut builder, wasm, None);
            builder.expect_success();
        }
    }
}

#[ignore]
#[test]
fn should_preserve_output_on_raw_curve_errors() {
    let mut builder = initialize();
    let checks = check_output(1024, &[0xa5; 64]);
    for (operation, size) in OPERATIONS {
        let mut input = vec![0; size];
        input[0] = 1;
        input[32] = 1;
        for input in [input, vec![0xff; size]] {
            let wasm = raw_wasm(
                operation,
                &input,
                0,
                size as u32,
                1024,
                u32::from(ApiError::InvalidArgument) as i32,
                &checks,
            );
            execute_raw(&mut builder, wasm, None);
            builder.expect_success();
        }
    }
}

#[ignore]
#[test]
fn should_report_raw_memory_access_errors() {
    let mut builder = initialize();
    for (operation, size) in OPERATIONS {
        let output_size = if operation == "pairing" { 1 } else { 64 };
        for (input_ptr, output_ptr) in [
            (65535, 1024),
            (u32::MAX - 31, 1024),
            (u32::MAX, 1024),
            (0, 65537 - output_size),
            (0, u32::MAX),
        ] {
            let wasm = raw_wasm(
                operation,
                &vec![0; size],
                input_ptr,
                size as u32,
                output_ptr,
                0,
                "",
            );
            execute_raw(&mut builder, wasm, None);
            builder.expect_failure();
            assert!(matches!(
                builder.get_error(),
                Some(Error::Exec(ExecError::Interpreter(_)))
            ));
        }
    }
}

#[ignore]
#[test]
fn should_write_fixed_outputs_and_allow_overlapping_buffers() {
    let mut builder = initialize();
    for (operation, input, expected) in [
        (
            "add",
            coordinates(&["1", "2", "0", "0"]),
            bytes(&coordinates(&["1", "2"])),
        ),
        (
            "mul",
            coordinates(&["1", "2", "1"]),
            bytes(&coordinates(&["1", "2"])),
        ),
        ("pairing", vec![[0; 32]; 6], vec![1]),
    ] {
        let input = bytes(&input);
        for output_ptr in [32, 1024, 65536 - expected.len() as u32] {
            let mut checks = check_output(output_ptr, &expected);
            if output_ptr == 1024 {
                checks.push_str(&check_output(output_ptr + expected.len() as u32, &[0xa5]));
            }
            let wasm = raw_wasm(
                operation,
                &input,
                0,
                input.len() as u32,
                output_ptr,
                0,
                &checks,
            );
            execute_raw(&mut builder, wasm, None);
            builder.expect_success();
        }
    }
}

#[ignore]
#[test]
fn should_charge_exact_gas_and_enforce_limits_before_memory_access() {
    let mut builder = initialize_with_curve_costs();
    for (operation, size, expected) in [
        ("add", 128, 1_328_000),
        ("mul", 96, 3_096_000),
        ("pairing", 0, 2_000_000),
        ("pairing", 192, 155_600_000),
        ("pairing", 384, 309_200_000),
    ] {
        let input = vec![0; size];
        let wasm = raw_wasm(operation, &input, 0, size as u32, 1024, 0, "");
        execute_raw(&mut builder, wasm, Some(expected));
        builder.expect_success();
        assert_eq!(builder.last_exec_gas_consumed(), Gas::new(expected));
        // Both pointers are deliberately out of bounds. Gas exhaustion must win.
        let wasm = raw_wasm(operation, &input, u32::MAX, size as u32, u32::MAX, 0, "");
        execute_raw(&mut builder, wasm, Some(expected - 1));
        builder.expect_failure();
        assert!(matches!(
            builder.get_error(),
            Some(Error::Exec(ExecError::GasLimit))
        ));
    }
}

#[ignore]
#[test]
fn should_charge_nonzero_pairing_batches_and_enforce_exact_limits() {
    let mut builder = initialize_with_curve_costs();
    let pair = pairing_vector();
    for (count, expected_gas, expected_result) in [
        (1, 155_600_000, 0),
        (2, 309_200_000, 1),
        (4, 616_400_000, 1),
    ] {
        let input = if count == 1 {
            bytes(&pair[..6])
        } else {
            bytes(&pair).repeat(count / 2)
        };
        let checks = check_output(1024, &[expected_result]);
        let wasm = raw_wasm("pairing", &input, 0, input.len() as u32, 1024, 0, &checks);
        execute_raw(&mut builder, wasm.clone(), Some(expected_gas));
        builder.expect_success();
        assert_eq!(builder.last_exec_gas_consumed(), Gas::new(expected_gas));

        execute_raw(&mut builder, wasm, Some(expected_gas - 1));
        builder.expect_failure();
        assert!(matches!(
            builder.get_error(),
            Some(Error::Exec(ExecError::GasLimit))
        ));
    }
}
