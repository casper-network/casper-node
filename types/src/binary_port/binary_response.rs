#[repr(u8)]
pub enum PayloadType {
    BlockHeader,
    BlockBody,
}

struct BinaryResponse {
    /// single byte - binary protocol version
    protocol_version: u8,
    /// single byte - return value - 0-ok, 1..255 error
    /// if not 0, no more bytes will follow
    error: u8,
    /// returned data type - u8 repr enum
    returned_data_type: u16,
    /// returned data type - serialization method (0 - bincode, 1 - bytesrepr)
    returned_data_serialization: u8,
    /// bytesrepr serialized original binary request
    request: Vec<u8>,
}
