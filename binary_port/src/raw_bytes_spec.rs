/// Stores raw bytes along with the flag indicating whether data is in a legacy format or not.
#[derive(Debug)]
pub struct RawBytesSpec {
    is_legacy: bool,
    raw_bytes: Vec<u8>,
}

impl RawBytesSpec {
    /// Creates an instance of the appropriate variant.
    pub fn new(raw_bytes: &[u8], is_legacy: bool) -> Self {
        Self {
            is_legacy,
            raw_bytes: raw_bytes.to_vec(),
        }
    }

    /// Creates a variant indicating that raw bytes are coming from a legacy source.
    pub fn new_legacy(raw_bytes: &[u8]) -> Self {
        Self {
            is_legacy: true,
            raw_bytes: raw_bytes.to_vec(),
        }
    }

    /// Creates a variant indicating that raw bytes are coming from the current database.
    pub fn new_current(raw_bytes: &[u8]) -> Self {
        Self {
            is_legacy: false,
            raw_bytes: raw_bytes.to_vec(),
        }
    }

    /// Is legacy?
    pub fn is_legacy(&self) -> bool {
        self.is_legacy
    }

    /// Raw bytes.
    pub fn raw_bytes(&self) -> Vec<u8> {
        self.raw_bytes.clone()
    }
}
