# casper-client

A client for interacting with the Casper network.


## Running the client

The client runs in one of several modes, each mode performing a single action. To see all available commands:

```
cd client
cargo run --release -- help
```

<details><summary>example output</summary>

```commandline
Casper client 1.5.0
A client for interacting with the Casper network

USAGE:
    casper-client [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    put-deploy             Creates a deploy and sends it to the network for execution
    make-deploy            Creates a deploy and outputs it to a file or stdout. As a file, the deploy can
                           subsequently be signed by other parties using the 'sign-deploy' subcommand and then sent
                           to the network for execution using the 'send-deploy' subcommand
    sign-deploy            Reads a previously-saved deploy from a file, cryptographically signs it, and outputs it
                           to a file or stdout
    send-deploy            Reads a previously-saved deploy from a file and sends it to the network for execution
    transfer               Transfers funds between purses
    get-deploy             Retrieves a deploy from the network
    get-block              Retrieves a block from the network
    list-deploys           Retrieves the list of all deploy hashes in a given block
    get-state-root-hash    Retrieves a state root hash at a given block
    query-state            Retrieves a stored value from the network
    get-balance            Retrieves a purse's balance from the network
    get-auction-info       Retrieves the bids and validators as of the most recently added block
    keygen                 Generates account key files in the given directory
    generate-completion    Generates a shell completion script
    help                   Prints this message or the help of the given subcommand(s)
```
</details>

To get further info on any command, run `help` followed by the subcommand, e.g.

```
cargo run --release -- help keygen
```

<details><summary>example output</summary>

```commandline
casper-client-keygen 
Generates account key files in the given directory. Creates ["secret_key.pem", "public_key.pem", "public_key_hex"].
"public_key_hex" contains the hex-encoded key's bytes with the hex-encoded algorithm tag prefixed

USAGE:
    casper-client keygen [FLAGS] [OPTIONS] [PATH]

FLAGS:
    -f               If this flag is passed, any existing output files will be overwritten. Without this flag, if any
                     output file exists, no output files will be generated and the command will fail
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -a, --algorithm <STRING>    The type of keys to generate [default: Ed25519]  [possible values: Ed25519, secp256k1]

ARGS:
    <PATH>    Path to output directory where key files will be created. If the path doesn't exist, it will be
              created. If not set, the current working directory will be used
```
</details>


### Generate asymmetric signing keys

Some commands require the use of a secret key for signing data. To generate a secret and public key pair:

```
cargo run --release -- keygen $HOME/.client_keys
```


## Interacting with a local node

Many client commands require to send HTTP requests and receive responses. To do this with a local node running on the
same machine, follow the instructions in [the `nctl` README](../utils/nctl/README.md) to set up a local test network.

Ensure the network has fully started before running client commands. This can be determined by running
`nctl-view-node-peers` and checking each node has connections to all others.

For client commands requiring a node address (specified via the `--node-address` or `-n` arg), the default value is
`http://localhost:50101`, which should match the address of the first node of a testnet started via `nctl`, and thus
can usually be omitted.


### Transfer funds between purses

The testnet will be set up so that the nodes each have an initial balance of tokens in their main purses. Let's say we
want to create a new purse under the public key we just created (in the "Generate asymmetric signing keys" section). We
can do this by creating a new deploy which will transfer funds between two purses once executed. The simplest way to
achieve this is via the `transfer` subcommand.

First, set the contents of the `public_key_hex` file to a variable. We'll use this as the target account:

```
PUBLIC_KEY=$(cat $HOME/.client_keys/public_key_hex)
```

Then execute the `transfer` subcommand. We'll specify that we want to transfer 1,234,567 tokens from the main purse of
node 3, and that we'll pay a maximum of 10,000 tokens to execute this deploy: 

```
cargo run --release -- transfer \
    --secret-key=../utils/nctl/assets/net-1/nodes/node-3/keys/secret_key.pem \
    --amount=1234567 \
    --target-account=$PUBLIC_KEY \
    --chain-name=casper-net-1 \
    --payment-amount=10000
```

<details><summary>example output</summary>

```commandline
{
  "jsonrpc": "2.0",
  "result": {
    "api_version": "1.0.0",
    "deploy_hash": "c42210759368a07a1b1ff4f019f7e77e7c9eaf2961b8c9dfc4237ea2218246c9"
  },
  "id": 2564730065
}
```
</details>

The `deploy_hash` in the response is worth noting, as it can be used to identify this deploy.


### Get details of a deploy

To see information about a deploy sent to the network via `transfer`, `put-deploy`, or `send-deploy`, you can use
`get-deploy`, along with the deploy hash printed after executing one of these subcommands.

For example, to see if our previous `transfer` command generated a deploy which was executed by the network:

```
cargo run --release -- get-deploy c42210759368a07a1b1ff4f019f7e77e7c9eaf2961b8c9dfc4237ea2218246c9
```

<details><summary>example output</summary>

```commandline
{
  "jsonrpc": "2.0",
  "result": {
    "api_version": "1.0.0",
    "deploy": {
      "approvals": [
        {
          "signature": "0140850c4f74aaad24894ce2d0e3efb64f599633fad4e280f39529dbd062ab49ca6a1f0bd6f20a8cddeab68e95ae5ea416a5b2ae3a02a0bc7a714c2915106e1c09",
          "signer": "015b7723f1d9499fa02bd17dfe4e1315cfe1660a071e27ab1f29d6ceb6e2abcd73"
        }
      ],
      "hash": "c42210759368a07a1b1ff4f019f7e77e7c9eaf2961b8c9dfc4237ea2218246c9",
      "header": {
        "account": "015b7723f1d9499fa02bd17dfe4e1315cfe1660a071e27ab1f29d6ceb6e2abcd73",
        "body_hash": "c66f1040f8f2aeafee73b7c0811e00fd6eb63a6a5992d7cc0f967e14704dd35b",
        "chain_name": "casper-net-1",
        "dependencies": [],
        "gas_price": 10,
        "timestamp": "2020-10-15T13:23:45.355Z",
        "ttl": "1h"
      },
      "payment": {
        "ModuleBytes": {
          "args": "0100000006000000616d6f756e740300000002102708",
          "module_bytes": ""
        }
      },
      "session": {
        "Transfer": {
          "args": "0200000006000000616d6f756e74040000000387d612080600000074617267657420000000018189fd2d42c36d951f9803e595795a3a0fc07aa999c88a28d286c7cbf338940f0320000000"
        }
      }
    },
    "execution_results": [
      {
        "block_hash": "80a09df67f45bfb290c8f36021daf2fb898587a48fa0e4f7c506202ae8f791b8",
        "result": {
          "cost": "0",
          "effect": {
            "operations": {
              "account-hash-018189fd2d42c36d951f9803e595795a3a0fc07aa999c88a28d286c7cbf33894": "Write",
              "hash-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db": "Write",
              "hash-d46e35465520ef9f868be3f26eaded1585dd66ac410706bab4b7adf92bdf528a": "Read",
              "hash-ea274222cc975e4daec2cced17a0270df7c282e865115d98f544a35877af5271": "Add",
              "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-000": "Write",
              "uref-8e7893be4b33bc5eacde4dd684b030593200364a211b8566ed9458ccbafbcde9-000": "Write",
              "uref-b645152645faa6c3f7708fd362a118296f7f4d39dc065c120877d13b6981cd67-000": "Write"
            },
            "transforms": {
              "account-hash-018189fd2d42c36d951f9803e595795a3a0fc07aa999c88a28d286c7cbf33894": "WriteAccount",
              "hash-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db": {
                "WriteCLValue": {
                  "bytes": "02b645152645faa6c3f7708fd362a118296f7f4d39dc065c120877d13b6981cd6707",
                  "cl_type": "Key"
                }
              },
              "hash-d46e35465520ef9f868be3f26eaded1585dd66ac410706bab4b7adf92bdf528a": "Identity",
              "hash-ea274222cc975e4daec2cced17a0270df7c282e865115d98f544a35877af5271": {
                "AddKeys": {
                  "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-000": "uref-b645152645faa6c3f7708fd362a118296f7f4d39dc065c120877d13b6981cd67-007"
                }
              },
              "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-000": {
                "WriteCLValue": {
                  "bytes": "",
                  "cl_type": "Unit"
                }
              },
              "uref-8e7893be4b33bc5eacde4dd684b030593200364a211b8566ed9458ccbafbcde9-000": {
                "WriteCLValue": {
                  "bytes": "087929775d78456301",
                  "cl_type": "U512"
                }
              },
              "uref-b645152645faa6c3f7708fd362a118296f7f4d39dc065c120877d13b6981cd67-000": {
                "WriteCLValue": {
                  "bytes": "0387d612",
                  "cl_type": "U512"
                }
              }
            }
          },
          "error_message": null
        }
      }
    ]
  },
  "id": 592430140
}
```
</details>

The `block_hash` in the response's `execution_results` is worth noting, as it can be used to identify the block in which
the deploy is included. If the deploy was successfully received and parsed by the node, but failed to execute, the
`error_message` in `execution_results` may provide useful information.


### Get details of a `Block`

To see information about a `Block` created by the network, you can use `get-block`. For example:

```
cargo run --release -- get-block --block-hash=80a09df67f45bfb290c8f36021daf2fb898587a48fa0e4f7c506202ae8f791b8
```

<details><summary>example output</summary>

```commandline
{
  "jsonrpc": "2.0",
  "result": {
    "api_version": "1.0.0",
    "block": {
      "body": null,
      "hash": "80a09df67f45bfb290c8f36021daf2fb898587a48fa0e4f7c506202ae8f791b8",
      "header": {
        "accumulated_seed": "e8c65524331dc950d9065c289deb05458d3f9d8beba15e663a5418f5a6c7bed5",
        "body_hash": "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8",
        "deploy_hashes": [
          "c42210759368a07a1b1ff4f019f7e77e7c9eaf2961b8c9dfc4237ea2218246c9"
        ],
        "era_end": null,
        "era_id": 89,
        "state_root_hash": "c79f4c9a017532fe265593d86d3917581479fd1601093e16d17ec90aeaa63b83",
        "height": 987,
        "parent_hash": "ffb95eac42eae1112d37797a1ecc67860e88a9364c44845cb7a96eb426dca502",
        "proposer": "015b7723f1d9499fa02bd17dfe4e1315cfe1660a071e27ab1f29d6ceb6e2abcd73",
        "random_bit": true,
        "timestamp": "2020-10-15T13:23:48.352Z"
      },
      "proofs": [
        "0104df3fe39567d22a48b68c4b046dadf5af6552c45b1a93613c89a65caa98b12a4564ba1a794e77787eb3d37c19617ca344f2a304387a0364fee0e8f89da2da0d"
      ]
    }
  },
  "id": 3484548969
}
```
</details>

The `state_root_hash` in the response's `header` is worth noting, as it can be used to identify the state root hash
for the purposes of querying the global state.


### Query the global state

To view data stored to global state after executing a deploy, you can use `query-state`. For example, to see the value
stored under our new account's public key:

```
cargo run --release -- query-state \
    --state-root-hash=242666f5959e6a51b7a75c23264f3cb326eecd6bec6dbab147f5801ec23daed6 \
    --key=$PUBLIC_KEY
```

<details><summary>example output</summary>

```commandline
{
  "jsonrpc": "2.0",
  "result": {
    "api_version": "1.0.0",
    "stored_value": {
      "Account": {
        "account_hash": "018189fd2d42c36d951f9803e595795a3a0fc07aa999c88a28d286c7cbf33894",
        "action_thresholds": {
          "deployment": 1,
          "key_management": 1
        },
        "associated_keys": [
          {
            "account_hash": "018189fd2d42c36d951f9803e595795a3a0fc07aa999c88a28d286c7cbf33894",
            "weight": 1
          }
        ],
        "main_purse": "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007",
        "named_keys": {}
      }
    }
  },
  "id": 3649040235
}
```
</details>

This yields details of the newly-created account object, including the `URef` of the account's main purse.


### Get the balance of a purse

This can be done via `get-balance`. For example, to get the balance of the main purse of our newly-created account:

```
cargo run --release -- get-balance \
    --state-root-hash=242666f5959e6a51b7a75c23264f3cb326eecd6bec6dbab147f5801ec23daed6 \
    --purse-uref=uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007
```

<details><summary>example output</summary>

```commandline
{
  "jsonrpc": "2.0",
  "result": {
    "api_version": "1.0.0",
    "balance_value": "1234567"
  },
  "id": 4193583276
}
```
</details>

Note that the system mint contract is required to retrieve the balance of any given purse. If you execute a
`query-state` specifying a purse `URef` as the `--key` argument, you'll find that the actual value stored there is a
unit value `()`. This makes the `get-balance` subcommand particularly useful. 

---


## Client library

The `lib` directory contains source for the client library, which may be called directly rather than through the CLI
binary. The CLI app `casper-client` makes use of this library to implement its functionality.


## Client library C wrapper

An optional feature of the client library is to use `cbindgen` to build a C wrapper for functions in the library. This
can then be leveraged to build bindings for the library in any language that can access an `extern "C"` interface.

The feature is named `ffi` and is enabled by default.

See `examples/ffi/README.md` for more information.
