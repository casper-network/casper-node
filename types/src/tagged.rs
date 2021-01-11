/// The quality of having a tag
pub trait Tagged<T> {
    /// Returns the tag of a given object
    fn tag(&self) -> T;
}
