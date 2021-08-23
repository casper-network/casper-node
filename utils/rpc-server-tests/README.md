# rpc-server-tests
Tool for testing the RPC server. Each test sends the pre-baked query and asserts if:
* response JSON matches the expected JSON schema
* HTTP error code
* RPC server error code in case of sad-path tests

# How to use
Run `cargo run -- --help` for help on how to run a single test case.

For example:
```
$ cargo run -- --node-address 172.16.0.8:11101 --test-directory test_suites/err/unknown_method/
    Finished dev [unoptimized + debuginfo] target(s) in 0.12s
     Running `/home/magister/Casper/casper-node/target/debug/rpc-server-tests --node-address '172.16.0.8:11101' --test-directory test_suites/err/unknown_method/`
Exit code: 1
Error code mismatch. Expected '-32600' got '-32601'
```

Alternatively, you can run all test suites with `cargo test` like so:
```
$ RPC_SERVER_TESTS_NODE_ADDRESS=172.16.0.8:11101 cargo test
    Finished test [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests (/home/magister/Casper/casper-node/target/debug/deps/rpc_server_tests-50c3ab21394183ae)

running 3 tests
test tests::ok::info_get_status ... ok
test tests::err::random_request_body ... ok
test tests::err::unknown_method ... FAILED

failures:

---- tests::err::unknown_method stdout ----
thread 'tests::err::unknown_method' panicked at 'assertion failed: `(left == right)`
  left: `1`,
 right: `0`', utils/rpc-server-tests/src/main.rs:78:9
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tests::err::unknown_method

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```