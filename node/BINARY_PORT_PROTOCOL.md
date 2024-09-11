# The Binary Port Protocol
This page specifies the communication protocol between the [RPC Sidecar](https://github.com/casper-network/casper-sidecar) and a Casper node's binary port.

## Synopsis
The communication protocol between the Sidecar and the binary port is a binary protocol that follows a simple request-response model. The protocol consists of one party (the client) sending requests to another party (the server) and the server sending responses back to the client. Both requests and responses are wrapped in envelopes containing a version and a payload type tag. The versioning scheme is based on [SemVer](https://semver.org/). See [versioning](#versioning) for more details. The payload type tags are used to interpret the contents of the payloads.

### Request format
| Size in bytes | Field                             | Description                                                                                                                                                                                                                                                                                       |
|---------------|-----------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| 2             | Version of the binary port header | Version of the binary port header serialized as a single u16 number. Upon receiving the request, the binary port component will first read data from this field and check it against the currently supported version. In case of a version mismatch, the appropriate error response will be sent. |
| 12            | Chain protocol version            | Chain protocol version as a u32 triplet (major, minor, patch). This parameter is used to determine whether an incoming request is compatible (according to semver rules) with the current chain protocol version. If not, the appropriate error response will be sent.                            |
| 1             | BinaryRequestTag                  | Tag identifying the request.                                                                                                                                                                                                                                                                      |
| Variable      | RequestPayload                    | Payload to be interpreted according to the `BinaryRequestTag`.                                                                                                                                                                                                                                    |

Request bytes can be constructed from the bytesrepr-serialized `BinaryRequestHeader` followed by the bytesrepr-serialized `BinaryRequest`.



### Response format
| Size in bytes   | Field           | Description                                                              |
|-----------------|-----------------|--------------------------------------------------------------------------|
| 2               | Request ID      | Request ID as a u16 number.                                              |
| 4               | LengthOfRequest | Length of the request (encoded as bytes) for this response.              |
| LengthOfRequest | RequestBytes    | The request, encoded as bytes, corresponding to this response.           |
| 12              | ProtocolVersion | Protocol version as a u32 triplet (major, minor, patch).                 |
| 2               | ErrorCode       | Error code, where 0 indicates success.                                   |
| 1-2             | ResponseType     | Optional payload type tag (first byte being 1 indicates that it exists).|
| Variable        | Payload         | Payload to be interpreted according to the `PayloadTag`.                 |

`BinaryResponseAndRequest` object can be bytesrepr-deserialized from these bytes.

**Notes:** `Variable` means that the payload size is variable and depends on the tag.

## Versioning
Versioning is based on the protocol version of the Casper Platform. The request/response model was designed to support **backward-compatible** changes to some parts, which are allowed to change between **MINOR** versions:
- addition of a new [`BinaryRequestTag`](#request-format) with its own payload
- addition of a new [`ResponseType`](#response-format) with its own payload
- addition of a new [`RecordId`](#request-model-details)
- addition of a new [`InformationRequestTag`](#request-model-details)
- addition of a new [`ErrorCode`](#response-format)

Implementations of the protocol can handle requests/responses with a different **MINOR** version than their own. It is possible that they receive a payload they don't support if their version is lower. In that case, they should respond with an error code indicating the lack of support for the given payload (`ErrorCode::UnsupportedRequest`).

Other changes to the protocol, such as changes to the format of existing requests/responses or removal of existing requests/responses, are only allowed between **MAJOR** versions. Implementations of the protocol should not handle requests/responses with a different **MAJOR** version than their own and immediately respond with an error code indicating the lack of support for the given version (`ErrorCode::UnsupportedRequest`).

Changes to the envelopes (the request/response headers) are allowed but are breaking. When such a change is required, the "Header version" should in the request header should also be changed to prevent binary port from trying to handle requests it can't process.

## Request model details
Currently, there are 3 supported types of requests, but the request model can be extended with new variants according to the [versioning](#versioning) rules. The request types are:
- A `Get` request, which is one of:
    - A `Record` request asking for a record with an [**extensible**](#versioning) `RecordId` tag and a key
    - An `Information` request asking for a piece of information with an [**extensible**](#versioning) `InformationRequestTag` tag and a key
    - A `State` request asking for some data from global state. This can be:
        - An `Item` request asking for a single item given a `Key`
        - An `AllItems` request asking for all items given a `KeyTag`
        - A `Trie` request asking for a trie given a `Digest`
- A `TryAcceptTransaction` request for a transaction to be accepted and executed
- A `TrySpeculativeExec` request for a transaction to be executed speculatively, without saving the transaction effects in global state
