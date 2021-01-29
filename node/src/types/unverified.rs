use serde::Deserialize;

/// An unverified object.
///
/// Unverified objects come from "hostile" sources like other nodes, external clients and need to
/// validated before they are processed, forwarded or trusted in any other form.
///
/// The only guarantee is that they are structurally well-formed.
#[derive(Clone, Debug, Deserialize)]
pub struct Unverified<T>(T);

impl<T> Unverified<T> {
    /// Returns a readable reference to the held object.
    pub fn unverified_ref(&self) -> &T {
        &self.0
    }

    /// Marks an unverified object as validated.
    pub fn has_been_verified(self) -> T {
        self.0
    }
}
