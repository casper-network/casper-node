# Hello World Assembly Script

This assembly script accepts a message string and stores it in the calling account under the `special_value` NamedKey.

**Usage**: This assembly script expects a runtime argument named `message` of type string.

**Tests**: There are two tests available to test the Hello World assembly script. The `should_store_hello_world` test verifies the happy path, where a string *hello world* is saved under the `special_value` NamedKey. The `should_error_on_missing_runtime_arg` test verifies that an error is displayed when the runtime argument is missing. 
The tests start by initializing the Casper crates and creating a genesis account. Then the contract Wasm is loaded and the deploy_item object is created. The deploy_item object is passed to the execute_request. Finally, the execution engine is invoked to process the execute_request. 

## Build and Test the Assembly Script

### Set up the Rust toolchain
You need the Rust toolchain to develop smart contracts.
```bash
make setup
```

### Compile smart contracts
Compile WebAssembly (Wasm) files that will be used to deploy the assembly script.
```bash
make build-contracts
```
or
```bash
make build-contracts-as
```

The command `make build-contracts` compiles rust as well as assembly script. Whereas, `make build-contracts-as` will compile only assembly script.

### Test
Run the tests suite.
```bash
make test-contracts
```
or
```bash
make test-contracts-as
```
The command `make test-contracts` tests rust as well as assembly Wasm. Whereas, `make test-contracts-as` will test only assembly Wasm.

## Deploy the Hello World Assembly Script

You can deploy the Hello World assembly script on a local network using NCTL. For more information on how to run an NCTL network, see [Setting up an NCTL network](https://docs.casperlabs.io/dapp-dev-guide/building-dapps/setup-nctl/).

This command provides a view into the faucet account details. The faucet is the default account created on the NCTL network.

```bash
nctl-view-faucet-account
```

<details>
<summary>Sample faucet account details</summary>

```bash
2022-06-21T10:06:56.354497 [INFO] [226848] NCTL :: faucet a/c secret key    : /home/ubuntu/casper-node/utils/nctl/assets/net-1/faucet/secret_key.pem
2022-06-21T10:06:56.356638 [INFO] [226848] NCTL :: faucet a/c key           : 0146e3d5235a9c895d22eba969f390d0217aedbe0b8abcda0ea7ed0c27b3cd9d36
2022-06-21T10:06:56.358489 [INFO] [226848] NCTL :: faucet a/c hash          : ba94ea5ab81adceb47ca4a0502f926633b5643d90beae3bdf3f55117ceaf2297
2022-06-21T10:06:56.360308 [INFO] [226848] NCTL :: faucet a/c purse         : uref-bc30bba7e0701b726e89c08154b6037a8bc4b0bb8635595fb254f6081f0da0b1-007
2022-06-21T10:06:56.362112 [INFO] [226848] NCTL :: faucet a/c purse balance : 1000000000000000000000000000000000
2022-06-21T10:06:56.363781 [INFO] [226848] NCTL :: faucet on-chain account  : see below

{
  "api_version": "1.0.0",
  "block_header": null,
  "merkle_proof": "[2160 hex chars]",
  "stored_value": {
    "Account": {
      "account_hash": "account-hash-ba94ea5ab81adceb47ca4a0502f926633b5643d90beae3bdf3f55117ceaf2297",
      "action_thresholds": {
        "deployment": 1,
        "key_management": 1
      },
      "associated_keys": [
        {
          "account_hash": "account-hash-ba94ea5ab81adceb47ca4a0502f926633b5643d90beae3bdf3f55117ceaf2297",
          "weight": 1
        }
      ],
      "main_purse": "uref-bc30bba7e0701b726e89c08154b6037a8bc4b0bb8635595fb254f6081f0da0b1-007",
      "named_keys": []
    }
  }
}
```

</details>

The following command will help you deploy the assembly script on the NCTL network. In the following command, the KEY PATH is the path of the faucet account secret key.

```bash
casper-client put-deploy \
    --node-address http://localhost:11101 \
    --chain-name casper-net-1 \
    --secret-key [KEY PATH]/secret_key.pem \
    --payment-amount 5000000000000 \
    --session-path [CONTRACT PATH]/contract.wasm \
    --session-arg "message:string='hello world'"    
```

After the deploy is successful, you can view the new NamedKey `special_value` in the faucet account details.  

<details>
<summary>Sample faucet account details after successful deploy</summary>

```bash
2022-06-21T10:11:49.190801 [INFO] [226848] NCTL :: faucet a/c secret key    : /home/ubuntu/casper-node/utils/nctl/assets/net-1/faucet/secret_key.pem
2022-06-21T10:11:49.192818 [INFO] [226848] NCTL :: faucet a/c key           : 0146e3d5235a9c895d22eba969f390d0217aedbe0b8abcda0ea7ed0c27b3cd9d36
2022-06-21T10:11:49.194613 [INFO] [226848] NCTL :: faucet a/c hash          : ba94ea5ab81adceb47ca4a0502f926633b5643d90beae3bdf3f55117ceaf2297
2022-06-21T10:11:49.196407 [INFO] [226848] NCTL :: faucet a/c purse         : uref-bc30bba7e0701b726e89c08154b6037a8bc4b0bb8635595fb254f6081f0da0b1-007
2022-06-21T10:11:49.198393 [INFO] [226848] NCTL :: faucet a/c purse balance : 999999999999999999999900000000000
2022-06-21T10:11:49.200185 [INFO] [226848] NCTL :: faucet on-chain account  : see below
{
  "api_version": "1.0.0",
  "block_header": null,
  "merkle_proof": "[2330 hex chars]",
  "stored_value": {
    "Account": {
      "account_hash": "account-hash-ba94ea5ab81adceb47ca4a0502f926633b5643d90beae3bdf3f55117ceaf2297",
      "action_thresholds": {
        "deployment": 1,
        "key_management": 1
      },
      "associated_keys": [
        {
          "account_hash": "account-hash-ba94ea5ab81adceb47ca4a0502f926633b5643d90beae3bdf3f55117ceaf2297",
          "weight": 1
        }
      ],
      "main_purse": "uref-bc30bba7e0701b726e89c08154b6037a8bc4b0bb8635595fb254f6081f0da0b1-007",
      "named_keys": [
        {
          "key": "uref-07ea6a04bc49f73d20b78f81d994cd324b60ea62046cc73e7a2030f4c25c2759-007",
          "name": "special_value"
        }
      ]
    }
  }
}
```

</details>

:::note

Make a note of the NamedKey URef hash in the faucet account details.

:::

### View the value stored in the NamedKey

You can view the value stored in the NamedKey by using the URef hash assigned to the NamedKey. To do this, we will first find the state root hash of the network using the following command:

```bash
casper-client get-state-root-hash --node-address http://localhost:11101
```

<details>
<summary>Sample output with state root hash</summary>

```json
{
"id": -7547762796950564402,
  "jsonrpc": "2.0",
  "result": {
    "api_version": "1.0.0",
    "state_root_hash": "6097c728c1af6179a491b5c3d143c49b032c9d6bb69be553729cb9f1489c3833"
  }
}
```

</details>

Then use the URef hash and the state root hash to view the value stored in the NamedKey.

```bash
casper-client query-state \
> --node-address http://localhost:11101 \
> --key [NamedKey Uref Hash] \
> --state-root-hash [STATE ROOT HASH]
```

<details>
<summary>Sample output displaying the NamedKey value</summary>

```json
{
  "id": 5590727100213957592,
  "jsonrpc": "2.0",
  "result": {
    "api_version": "1.0.0",
    "block_header": null,
    "merkle_proof": "[3958 hex chars]",
    "stored_value": {
      "CLValue": {
        "bytes": "0b00000068656c6c6f20776f726c64",
        "cl_type": "String",
        "parsed": "hello world"
      }
    }
  }
}
```

</details>



