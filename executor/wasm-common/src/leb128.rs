use std::io::{self, Read, Write};

use borsh::{BorshDeserialize, BorshSerialize};

pub fn leb128_encode_to(mut value: u64, writer: &mut impl Write) -> io::Result<()> {
    loop {
        let mut byte = (value & 0x7F) as u8; // Take the lowest 7 bits
        value >>= 7; // Right shift the value by 7 bits
        if value != 0 {
            byte |= 0x80; // If there are more bytes to come, set the high-order bit
        }
        writer.write_all(&[byte])?;
        if value == 0 {
            break;
        }
    }
    Ok(())
}

pub fn leb128_decode(reader: &mut impl Read) -> io::Result<u64> {
    let mut result = 0;
    let mut shift = 0;
    let mut buffer = [0; 1]; // buffer to read a single byte
    loop {
        reader.read_exact(&mut buffer)?;
        let byte = buffer[0];
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
}

pub fn leb128_encode(value: u64) -> Vec<u8> {
    let mut buffer = Vec::new();
    leb128_encode_to(value, &mut buffer).expect("Failed to encode LEB128");
    buffer
}

pub struct LEB128U(u64);

impl LEB128U {
    pub const fn new(value: u64) -> Self {
        LEB128U(value)
    }
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

impl BorshSerialize for LEB128U {
    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        leb128_encode_to(self.0, writer)
    }
}

impl BorshDeserialize for LEB128U {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        let value = leb128_decode(reader)?;
        Ok(LEB128U(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leb128_encode_zero() {
        let value = 0;
        let expected = vec![0];
        assert_eq!(leb128_encode(value), expected);
    }

    #[test]
    fn test_leb128_encode_single_byte() {
        let value = 127;
        let expected = vec![127];
        assert_eq!(leb128_encode(value), expected);
    }

    #[test]
    fn test_leb128_encode_multiple_bytes() {
        let value = 300;
        let expected = vec![172, 2];
        assert_eq!(leb128_encode(value), expected);
    }

    #[test]
    fn test_leb128_encode_max_value() {
        let value = u64::max_value();
        let expected = vec![255, 255, 255, 255, 255, 255, 255, 255, 255, 1];
        let encoded = leb128_encode(value);
        assert_eq!(encoded, expected);
    }
}
