#[derive(Clone, Debug)]
struct Unverified<T>(T);

impl<T> Unverified<T> {
    fn unverified_ref(&self) -> &T {
        &self.0
    }

    fn has_been_verified(self) -> T {
        self.0
    }
}
