# Binary port protocol
The specification of the protocol used to communicate between the RPC sidecar and binary port casper-node.

## Synopsis
This is a binary protocol which follows a simple request-response model built on top of [Juliet](https://github.com/casper-network/juliet). The protocol consists of one party (the client) sending requests to another party (the server) and the server sending responses back to the client. Both requests and responses are wrapped in envelopes containing a version and a payload type tag. The versioning scheme is based on [SemVer](https://semver.org/), see [versioning](#versioning) for more details. The payload type tags are used to interpret the contents of the payloads.

### Request format
| Size in bytes | Field            | Description                                               |
|---------------|------------------|-----------------------------------------------------------|
| 12            | ProtocolVersion  | Protocol version as a u32 triplet (major, minor, patch)   |
| 1             | BinaryRequestTag | Tag identifying the request                               |
| ...           | RequestPayload   | Payload to be interpreted according to `BinaryRequestTag` |

Request bytes can be constructed from bytesrepr-serialized `BinaryRequestHeader` followed by bytesrepr-serialized `BinaryRequest`

### Response format
| Size in bytes   | Field           | Description                                                             |
|-----------------|-----------------|-------------------------------------------------------------------------|
| 4               | LengthOfRequest | Length of the request being responded to                                |
| LengthOfRequest | RequestBytes    | The request being responded to encoded as bytes                         |
| 12              | ProtocolVersion | Protocol version as a u32 triplet (major, minor, patch)                 |
| 1               | ErrorCode       | Error code, where 0 indicates success                                   |
| 1-2             | PayloadType     | Optional payload type tag (first byte being 1 indicates that it exists) |
| ...             | Payload         | Payload to be interpreted according to `PayloadTag`                     |

`BinaryResponseAndRequest` object can be bytesrepr-deserialized from these bytes.

**Notes:** `...` means that the payload size is variable in size and depends on the tag.

## Versioning
Versioning is based on the protocol version of the Casper Platform and the request/response model was designed to support **backwards-compatible** changes to some parts of it. These are allowed to change between **MINOR** versions:
- addition of new [`BinaryRequestTag`](#request-format) with its own payload
- addition of new [`PayloadType`](#response-format) with its own payload
- addition of new [`RecordId`](#request-model-details)
- addition of new [`InformationRequestTag`](#request-model-details)
- addition of new [`ErrorCode`](#response-format)

Implementations of the protocol can handle requests/responses with a different **MINOR** version than their own. It is possible that they receive a payload they don't support if their version is lower. In that case they should respond with an error code indicating the lack of support for the given payload (`ErrorCode::UnsupportedRequest`).

Other changes to the protocol such as changes to the format of existing requests/responses or removal of existing requests/responses are only allowed between **MAJOR** versions. Implementations of the protocol should not handle requests/responses with a different **MAJOR** version than their own and immediately respond with an error code indicating the lack of support for the given version (`ErrorCode::UnsupportedRequest`).

Changes to the envelopes (the request/response headers) are not allowed.

## Request model details
There are currently 3 supported types of requests, but the request model can be extended with new variants according to the [versioning](#versioning) rules. The request types are:
- `Get` request, which is one of:
    - `Record` request asking for a record with an [**extensible**](#versioning) `RecordId` tag and a key
    - `Information` request asking for a piece of information with an [**extensible**](#versioning) `InformationRequestTag` tag and a key
    - `State` request asking for some data from the global state
        - `Item` request asking for a single item by a `Key`
        - `AllItems` request asking for all items by a `KeyTag`
        - `Trie` request asking for a trie by a `Digest`
- `TryAcceptTransaction` request a transaction to be accepted and executed
- `TrySpeculativeExec` request a transaction to be speculatively executed
