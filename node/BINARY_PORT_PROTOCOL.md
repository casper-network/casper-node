# The Binary Port Protocol

This page specifies the protocol of casper nodes Binary Port.

## Synopsis

The protocol consists of one party (the client) sending requests to another party (the server) and the server sending responses back to the client.
The Binary Port communication protocol is binary and supports a long lived tcp connection. Once the tcp connection is open the binary port assumes a series of request-response messages. It is not supported to send a second request before receiving the entirety of the response to the first one via one tcp connection. Both requests and responses are have envelopes containing some metadata.

### Request format

| Size in bytes | Field    | Description                                                                                                                                                                                                                      |
| ------------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 4             | length   | A LE encoded number of bytes of all the subsequent fields (excluding the length itself). Based on this number the server "knows" where the binary request ends.                                                                  |
| 2             | version  | Version of the binary port header serialized as a single u16 number. The server handles only strictly specified versions and can deny service if the version doesn't meet it's expectation. The current supported version is `1` |
| 1             | type_tag | Tag identifying the request.                                                                                                                                                                                                     |
| 2             | id       | An identifier that should dbe understandable to the client and should facilitate correlating requests with responses                                                                                                             |
| variable      | payload  | Payload to be interpreted according to the `type_tag`.                                                                                                                                                                           |

### Response format

| Size in bytes | Field          | Description                                                                                                                                                                                 |
| ------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 4             | length         | A LE encoded number of bytes of all the subsequent fields (excluding the length itself). Based on this number the client should "know" where the binary response ends.                      |
| 4             | Request length | number of bytes of the `request` field.                                                                                                                                                     |
| variable      | request        | The raw binary request that was provided by the client (including the requests `length` field).                                                                                             |
| 2             | version        | Version of the binary port response structure. Currently supported version is `1`                                                                                                           |
| 2             | error_code     | Error code, where 0 indicates success.                                                                                                                                                      |
| 1-2           | response_type  | Optional payload type tag (first byte being 1 indicates that it exists).                                                                                                                    |
| 4             | payload_length | Number of bytes of the var-length `payload` field.                                                                                                                                          |
| Variable      | payload        | Payload to be interpreted according to the `response_type`. If there is no response, or the response was erroneous this field will have 0 bytes and `payload_length` will be the number `0` |

**Notes:** `variable` means that the payload size is variable and depends on the tag.

## Request model details

Currently, there are 3 supported types of requests, but the request model can be extended. The request types are:

- A `Get` request, which is one of:
  - A `Record` request asking for a record with an extensible `RecordId` tag and a key
  - An `Information` request asking for a piece of information with an extensible `InformationRequestTag` tag and a key
  - A `State` request asking for some data from global state. This can be:
    - An `Item` request asking for a single item given a `Key`
    - An `AllItems` request asking for all items given a `KeyTag`
    - A `Trie` request asking for a trie given a `Digest`
- A `TryAcceptTransaction` request for a transaction to be accepted and executed
- A `TrySpeculativeExec` request for a transaction to be executed speculatively, without saving the transaction effects in global state
