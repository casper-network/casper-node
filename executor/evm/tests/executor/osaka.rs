//! Regression vectors through the Casper executor, not just revm's standalone functions.
use super::*;
use casper_types::EVM_TRANSACTION_GAS_LIMIT;

fn precompile_address(number: u16) -> evm::Address {
    let mut bytes = [0; 20];
    bytes[18..].copy_from_slice(&number.to_be_bytes());
    evm::Address::new(bytes)
}

fn set_gas(request: &mut ExecuteRequest, gas_limit: u64) {
    let ExecuteKind::Call(call) = &mut request.kind else {
        panic!("expected call")
    };
    call.gas_limit = gas_limit;
}

fn input_gas(input: &[u8]) -> u64 {
    input
        .iter()
        .map(|byte| if *byte == 0 { 4 } else { 16 })
        .sum()
}

#[test]
fn clz_returns_leading_zero_count_at_five_gas() {
    let executor = executor(EvmSpec::Osaka);
    let (mut state, dal, _temp) = tracking_copy();
    let target = evm::Address::new([0x55; 20]);
    for (value, expected) in [
        (CasperU256::zero(), 256),
        (CasperU256::one(), 255),
        (CasperU256::from(0x100), 247),
        (CasperU256::one() << 255, 0),
        (CasperU256::MAX, 0),
    ] {
        let mut runtime = vec![opcode::PUSH32];
        let mut bytes = [0; 32];
        value.to_big_endian(&mut bytes);
        runtime.extend(bytes);
        runtime.extend([
            opcode::CLZ,
            opcode::PUSH0,
            opcode::MSTORE,
            opcode::PUSH1,
            32,
            opcode::PUSH0,
            opcode::RETURN,
        ]);
        seed_evm_code(&mut state, target, runtime);
        let output = execute_call(
            &executor,
            &dal,
            &mut state,
            evm::Address::ZERO,
            Some(target),
            Vec::new(),
        );
        assert_eq!(decode_word(&output.output), expected);
        // PUSH32 3 + CLZ 5 + PUSH0 2 + MSTORE 3 + memory 3 + PUSH1 3 + PUSH0 2.
        assert_eq!(output.gas_used, 21_021);
    }
}

fn p256_input() -> Vec<u8> {
    // d=1, k=1, z=42: Q=G, r=G.x and s=(r+42) mod n.
    let x = decode_hex("6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296");
    let y = decode_hex("4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
    let mut s = [0; 32];
    (CasperU256::from_big_endian(&x) + 42).to_big_endian(&mut s);
    [word(42).to_vec(), x.clone(), s.to_vec(), x, y].concat()
}

fn p256_order() -> CasperU256 {
    CasperU256::from_big_endian(&decode_hex(
        "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
    ))
}

#[test]
fn p256_verification_edges_cost_and_out_of_gas() {
    let executor = executor(EvmSpec::Osaka);
    let (state, dal, _temp) = tracking_copy();
    let valid = p256_input();
    let mut high_s = valid.clone();
    (p256_order() - CasperU256::from_big_endian(&valid[64..96])).to_big_endian(&mut high_s[64..96]);
    let mut vectors = vec![
        (valid.clone(), true),
        (high_s, true),
        (vec![], false),
        (vec![0; 160], false),
        (valid[..159].to_vec(), false),
        ([valid.clone(), vec![0]].concat(), false),
    ];
    for index in [0, 32, 64, 96, 128] {
        let mut bad = valid.clone();
        if index == 0 {
            bad[0] ^= 1;
        } else {
            bad[index..index + 32].fill(0);
        }
        vectors.push((bad, false));
    }
    for index in [32, 64] {
        let mut bad = valid.clone();
        p256_order().to_big_endian(&mut bad[index..index + 32]);
        vectors.push((bad, false));
    }
    // Q=-G, z=r=s=1 makes the verification point infinity; it must fail.
    let mut infinity = [
        word(1).to_vec(),
        word(1).to_vec(),
        word(1).to_vec(),
        valid[96..128].to_vec(),
        vec![0; 32],
    ]
    .concat();
    let prime = CasperU256::from_big_endian(&decode_hex(
        "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
    ));
    for index in [96, 128] {
        let mut noncanonical = valid.clone();
        prime.to_big_endian(&mut noncanonical[index..index + 32]);
        vectors.push((noncanonical, false));
    }
    (prime - CasperU256::from_big_endian(&valid[128..])).to_big_endian(&mut infinity[128..]);
    vectors.push((infinity, false));
    // The rare R.x >= n case requires comparison modulo the group order.
    vectors.push((modular_p256_input(), true));
    for (input, verifies) in vectors {
        let mut request = call_request(
            evm::Address::ZERO,
            Some(precompile_address(0x100)),
            input.clone(),
            CasperU256::zero(),
        );
        let intrinsic = 21_000 + input_gas(&input);
        set_gas(&mut request, intrinsic + 6_900);
        let result = executor
            .execute(&dal, &mut state.fork(), request.clone())
            .unwrap();
        assert_eq!(result.status, ExecutionStatus::Success);
        assert_eq!(
            result.output,
            if verifies { word(1).to_vec() } else { vec![] }
        );
        assert_eq!(result.gas_used, intrinsic + 6_900);
        set_gas(&mut request, intrinsic + 6_899);
        let result = executor.execute(&dal, &mut state.fork(), request).unwrap();
        assert_eq!(
            result.status,
            ExecutionStatus::Halt(evm::HaltReason::OutOfGas(evm::OutOfGasError::Precompile))
        );
        assert_eq!(result.gas_used, intrinsic + 6_899);
    }
}

#[test]
fn advertised_precompiles_execute_and_p256_is_warm() {
    let executor = executor(EvmSpec::Osaka);
    let (mut state, dal, _temp) = tracking_copy();
    let advertised = executor.precompile_addresses();
    assert_eq!(advertised, executor.config().active_precompiles());
    assert_eq!(advertised.len(), 18);
    assert_eq!(advertised["P256VERIFY"], precompile_address(0x100));
    assert_eq!(advertised["KZG_POINT_EVALUATION"], precompile_address(10));
    for address in advertised.values() {
        // Empty inputs either succeed or halt within the precompile, never behave like an EOA.
        let request = call_request(
            evm::Address::ZERO,
            Some(*address),
            vec![],
            CasperU256::zero(),
        );
        let outcome = executor.execute(&dal, &mut state.fork(), request).unwrap();
        assert!(
            outcome.gas_used > 21_000,
            "{address} must execute its precompile"
        );
    }
    let target = evm::Address::new([0x55; 20]);
    seed_evm_code(
        &mut state,
        target,
        vec![
            opcode::PUSH2,
            1,
            0,
            opcode::BALANCE,
            opcode::POP,
            opcode::STOP,
        ],
    );
    let outcome = execute_call(
        &executor,
        &dal,
        &mut state,
        evm::Address::ZERO,
        Some(target),
        vec![],
    );
    assert_eq!(outcome.gas_used, 21_105); // PUSH 3 + warm BALANCE 100 + POP 2.
    let disabled = EvmExecutor::new(EvmConfig::default());
    assert!(disabled.precompile_addresses().is_empty());
    assert_eq!(
        disabled.precompile_addresses(),
        disabled.config().active_precompiles()
    );
}

fn modexp_input(lengths: [u64; 3]) -> Vec<u8> {
    lengths.into_iter().flat_map(word).collect()
}

#[test]
fn modexp_enforces_each_declared_1024_byte_boundary() {
    let executor = executor(EvmSpec::Osaka);
    let (state, dal, _temp) = tracking_copy();
    for index in 0..3 {
        for length in [1024, 1025] {
            let mut lengths = [1; 3];
            lengths[index] = length;
            let input = modexp_input(lengths); // Short bodies are zero padded after checking lengths.
            let outcome = executor
                .execute(
                    &dal,
                    &mut state.fork(),
                    call_request(
                        evm::Address::ZERO,
                        Some(precompile_address(5)),
                        input,
                        CasperU256::zero(),
                    ),
                )
                .unwrap();
            if length == 1024 {
                assert_eq!(outcome.status, ExecutionStatus::Success);
            } else {
                assert_eq!(
                    outcome.status,
                    ExecutionStatus::Halt(evm::HaltReason::PrecompileError)
                );
                assert_eq!(outcome.gas_used, 5_000_000);
            }
        }
    }
    let mut huge = modexp_input([1, 1, 1]);
    huge[..32].fill(0xff);
    let outcome = executor
        .execute(
            &dal,
            &mut state.fork(),
            call_request(
                evm::Address::ZERO,
                Some(precompile_address(5)),
                huge,
                CasperU256::zero(),
            ),
        )
        .unwrap();
    assert_eq!(
        outcome.status,
        ExecutionStatus::Halt(evm::HaltReason::PrecompileError)
    );
}

#[test]
fn modexp_osaka_exact_repricing_and_out_of_gas() {
    let executor = executor(EvmSpec::Osaka);
    let (state, dal, _temp) = tracking_copy();
    // The empty input exercises the new 500 minimum without a calldata floor.
    let result = execute_call(
        &executor,
        &dal,
        &mut state.fork(),
        evm::Address::ZERO,
        Some(precompile_address(5)),
        vec![],
    );
    assert_eq!(result.gas_used, 21_500);
    for (size, exp_len, expected) in [
        (32, 1, 500),
        (33, 33, 800),
        (128, 1, 512),
        (256, 1, 2048),
        (1024, 1, 32768),
        (128, 33, 8192),
    ] {
        let mut input = modexp_input([size, exp_len, size]);
        let mut base = vec![0; size as usize];
        *base.last_mut().unwrap() = 2;
        let mut exp = vec![0; exp_len as usize];
        *exp.last_mut().unwrap() = 2;
        let mut modulus = vec![0; size as usize];
        *modulus.last_mut().unwrap() = 5;
        input.extend(base);
        input.extend(exp);
        input.extend(modulus);
        let result = execute_call(
            &executor,
            &dal,
            &mut state.fork(),
            evm::Address::ZERO,
            Some(precompile_address(5)),
            input,
        );
        assert_eq!(result.output.last(), Some(&4));
        // Build input in the contract to measure frame gas independently of EIP-7623's
        // transaction calldata floor. Return [gas consumed around STATICCALL, success].
        for budget in [expected, expected - 1] {
            let mut runtime = vec![];
            for (offset, value) in [(0, size), (32, exp_len), (64, size)] {
                runtime.extend([
                    opcode::PUSH2,
                    (value >> 8) as u8,
                    value as u8,
                    opcode::PUSH1,
                    offset,
                    opcode::MSTORE,
                ]);
            }
            for (offset, value) in [
                (96 + size - 1, 2),
                (96 + size + exp_len - 1, 2),
                (96 + size + exp_len + size - 1, 5),
            ] {
                runtime.extend([
                    opcode::PUSH1,
                    value,
                    opcode::PUSH2,
                    (offset >> 8) as u8,
                    offset as u8,
                    opcode::MSTORE8,
                ]);
            }
            let input_len = 96 + size + exp_len + size;
            runtime.extend([
                opcode::GAS,
                opcode::PUSH0,
                opcode::PUSH0,
                opcode::PUSH2,
                (input_len >> 8) as u8,
                input_len as u8,
                opcode::PUSH0,
                opcode::PUSH1,
                5,
                opcode::PUSH3,
                (budget >> 16) as u8,
                (budget >> 8) as u8,
                budget as u8,
                opcode::STATICCALL,
                opcode::PUSH1,
                32,
                opcode::MSTORE,
                opcode::GAS,
                opcode::SWAP1,
                opcode::SUB,
                opcode::PUSH0,
                opcode::MSTORE,
                opcode::PUSH1,
                64,
                opcode::PUSH0,
                opcode::RETURN,
            ]);
            let mut view = state.fork();
            let target = evm::Address::new([0x55; 20]);
            seed_evm_code(&mut view, target, runtime);
            let result = execute_call(
                &executor,
                &dal,
                &mut view,
                evm::Address::ZERO,
                Some(target),
                vec![],
            );
            assert_eq!(decode_word(&result.output[..32]), budget + 123);
            assert_eq!(
                decode_word(&result.output[32..]),
                u64::from(budget == expected)
            );
        }
    }
}

#[test]
fn oversized_modexp_burns_forwarded_gas_and_returns_failure_to_caller() {
    let executor = executor(EvmSpec::Osaka);
    let (mut state, dal, _temp) = tracking_copy();
    let target = evm::Address::new([0x55; 20]);
    // Copy calldata, STATICCALL MODEXP with 100000 gas, and return the success flag.
    seed_evm_code(
        &mut state,
        target,
        vec![
            opcode::CALLDATASIZE,
            opcode::PUSH0,
            opcode::PUSH0,
            opcode::CALLDATACOPY,
            opcode::PUSH0,
            opcode::PUSH0,
            opcode::CALLDATASIZE,
            opcode::PUSH0,
            opcode::PUSH1,
            5,
            opcode::PUSH3,
            1,
            0x86,
            0xa0,
            opcode::STATICCALL,
            opcode::PUSH0,
            opcode::MSTORE,
            opcode::PUSH1,
            32,
            opcode::PUSH0,
            opcode::RETURN,
        ],
    );
    let result = execute_call(
        &executor,
        &dal,
        &mut state,
        evm::Address::ZERO,
        Some(target),
        modexp_input([1025, 1, 1]),
    );
    assert_eq!(decode_word(&result.output), 0);
    assert!(result.gas_used >= 121_000 && result.gas_used < 122_000);
}

fn capped_transaction(kind: u8, gas_limit: u64) -> EvmTransaction {
    let to = TxKind::Call(AlloyAddress::from([0x44; 20]));
    let signature = Signature::test_signature();
    let envelope: TxEnvelope = match kind {
        0 => TxLegacy {
            chain_id: Some(7),
            gas_limit,
            to,
            ..Default::default()
        }
        .into_signed(signature)
        .into(),
        1 => TxEip2930 {
            chain_id: 7,
            gas_limit,
            to,
            ..Default::default()
        }
        .into_signed(signature)
        .into(),
        2 => TxEip1559 {
            chain_id: 7,
            gas_limit,
            to,
            ..Default::default()
        }
        .into_signed(signature)
        .into(),
        4 => TxEip7702 {
            chain_id: 7,
            gas_limit,
            to: AlloyAddress::from([0x44; 20]),
            authorization_list: vec![signed_authorization(evm::Address::ZERO, 0)],
            ..Default::default()
        }
        .into_signed(signature)
        .into(),
        _ => unreachable!(),
    };
    EvmTransaction::from_signed_rlp(
        envelope.encoded_2718(),
        Timestamp::zero(),
        casper_types::TimeDiff::from_seconds(60),
    )
    .unwrap()
}

#[test]
fn checked_execution_caps_all_envelopes_and_unsigned_simulations_use_block_budget() {
    let executor = executor(EvmSpec::Osaka);
    let (state, dal, _temp) = tracking_copy();
    for kind in [0, 1, 2, 4] {
        for gas in [
            EVM_TRANSACTION_GAS_LIMIT - 1,
            EVM_TRANSACTION_GAS_LIMIT,
            EVM_TRANSACTION_GAS_LIMIT + 1,
        ] {
            let request = ExecuteRequest {
                block: block(),
                kind: ExecuteKind::Transaction(Box::new(capped_transaction(kind, gas))),
            };
            let result = executor.execute(&dal, &mut state.fork(), request);
            if gas <= EVM_TRANSACTION_GAS_LIMIT {
                assert!(result.is_ok(), "type {kind}, gas {gas}: {result:?}");
            } else {
                assert!(matches!(result, Err(Error::Revm(_))));
            }
        }
    }
    let target = evm::Address::new([0x55; 20]);
    let mut request = call_request(evm::Address::ZERO, Some(target), vec![], CasperU256::zero());
    set_gas(&mut request, EVM_TRANSACTION_GAS_LIMIT + 1);
    assert!(
        executor
            .execute(&dal, &mut state.fork(), request.clone())
            .unwrap()
            .status
            == ExecutionStatus::Success
    );
    let ExecuteKind::Call(call) = &mut request.kind else {
        unreachable!()
    };
    call.validation = CallValidation::Checked;
    assert!(matches!(
        executor.execute(&dal, &mut state.fork(), request),
        Err(Error::Revm(_))
    ));
    let lower = EvmExecutor::new(EvmConfig {
        block_gas_limit: 100_000,
        ..executor.config().clone()
    });
    let mut request = call_request(evm::Address::ZERO, Some(target), vec![], CasperU256::zero());
    set_gas(&mut request, 100_001);
    assert!(matches!(
        lower.execute(&dal, &mut state.fork(), request),
        Err(Error::Revm(_))
    ));
}

fn modular_p256_input() -> Vec<u8> {
    decode_hex("000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000030000000000000000000000000000000000000000000000000000000000000003ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632554484f0c0fda434ef0a808458914f328715d7a545e198ac7eee31dffe861b5d23f")
}

#[test]
fn read_only_execution_can_consume_more_than_the_transaction_cap() {
    let executor = executor(EvmSpec::Osaka);
    let (mut state, dal, _temp) = tracking_copy();
    let target = evm::Address::new([0x55; 20]);
    // 650000 loop iterations at 26 gas each consume 16.9M execution gas.
    seed_evm_code(
        &mut state,
        target,
        vec![
            opcode::PUSH3,
            0x09,
            0xeb,
            0x10,
            opcode::JUMPDEST,
            opcode::PUSH1,
            1,
            opcode::SWAP1,
            opcode::SUB,
            opcode::DUP1,
            opcode::PUSH1,
            4,
            opcode::JUMPI,
            opcode::POP,
            opcode::STOP,
        ],
    );
    let mut request = call_request(evm::Address::ZERO, Some(target), vec![], CasperU256::zero());
    set_gas(&mut request, executor.config().block_gas_limit);
    let result = executor
        .execute(&dal, &mut state.fork(), request.clone())
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::Success);
    assert!(result.gas_used > EVM_TRANSACTION_GAS_LIMIT);
    set_gas(&mut request, EVM_TRANSACTION_GAS_LIMIT);
    let result = executor.execute(&dal, &mut state.fork(), request).unwrap();
    assert_eq!(
        result.status,
        ExecutionStatus::Halt(evm::HaltReason::OutOfGas(evm::OutOfGasError::Basic))
    );
}
