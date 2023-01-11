//! Specimen support.
//!
//! Structs implementing the specimen trait allow for specific sample instances being created, such
//! as the biggest possible.

/// Supports returning a maximum size specimen.
pub(crate) trait LargestSpecimen {
    /// Return the largest possible specimen for this type.
    fn largest_specimen() -> Self;
}
