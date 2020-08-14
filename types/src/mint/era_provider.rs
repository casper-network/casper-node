use crate::auction::EraId;

/// Provider for obtaining current era id.
pub trait EraProvider {
    /// Returns current era id.
    fn read_era_id(&mut self) -> EraId;
}
