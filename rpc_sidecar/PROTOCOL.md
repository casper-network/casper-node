# rpc-sidecar protocol
The specification of the protocol used to communicate between the RPC sidecar and casper-node.

## Synopsis
This is a binary protocol which follows a simple request-response model built on top of [Juliet](https://github.com/casper-network/juliet). The protocol consists of one party (the client) sending requests to another party (the server) and the server sending responses back to the client. Both requests and responses are wrapped in envelopes containing a version and a payload type tag. The versioning scheme is based on [SemVer](https://semver.org/), see [versioning](#versioning) for more details. The payload type tags are used to interpret the contents of the payloads.

### Request format
| Size in bytes | Field           | Description                                             |
|---------------|-----------------|---------------------------------------------------------|
| 12            | ProtocolVersion | Protocol version as a u32 triplet (major, minor, patch) |
| 1             | RequestTag      | Tag identifying the request                             |
| ...           | RequestPayload  | Payload to be interpreted according to `RequestTag`     |

### Response format
| Size in bytes   | Field           | Description                                                             |
|-----------------|-----------------|-------------------------------------------------------------------------|
| 4               | LengthOfRequest | Length of the request being responded to                                | 
| LengthOfRequest | RequestBytes    | The request being responded to encoded as bytes                         |
| 12              | ProtocolVersion | Protocol version as a u32 triplet (major, minor, patch)                 |
| 1               | ErrorCode       | Error code, where 0 indicates success                                   |
| 1-2             | PayloadTag      | Optional payload type tag (first byte being 1 indicates that it exists) |
| ...             | Payload         | Payload to be interpreted according to `PayloadTypeTag`                 |

**Note:** `...` means that the payload size is variable in size and depends on the tag.

## Versioning
Every version of the protocol follows a standard SemVer MAJOR.MINOR.PATCH scheme.

The protocol supports **backwards-compatible** changes to some parts of the request/response model. These are allowed to change between **MINOR** versions and they're listed below:
- addition of new [`RequestTag`](#request-format) with it's own payload
- addition of new [`PayloadTypeTag`](#response-format) with it's own payload
- addition of new [`DbId`](#request-model-details)
- addition of new [`ErrorCode`](#response-format)

Implementations of the protocol can handle requests/responses with a  different **MINOR** version than their own. It is possible that they receive a payload they don't support if their version is lower. In that case they should respond with an error code indicating the lack of support for the given payload.

Other changes to the protocol such as changes to the format of existing requests/responses or removal of existing requests/responses are only allowed between **MAJOR** versions. Implementations of the protocol should not handle requests/responses with a different **MAJOR** version than their own and immediately respond with an error code indicating the lack of support for the given version.

Changes to the envelopes (the request/response headers) are not allowed. 

## Request model details
There are currently 3 supported types of requests, but the request model can be extended with new variants according to the [versioning](#versioning) rules. The request types are:
- `Get` request, which is either one of:
    - `Db` request asking for a database item with an [**extensible**](#versioning) `DbId` tag and a key
    - `NonPersistedData` request asking for a transient piece of data with a `NonPersistedData` query
    - `State` request asking for an item from the global state by a key, a state root hash and a path
    - `AllValues` request asking for all values in the global state under a given key tag and a state root hash
    - `Trie` request asking for a trie with a hash
- `TryAcceptTransaction` request a transaction to be accepted and executed
- `TrySpeculativeExec` request a transaction to be speculatively executed
