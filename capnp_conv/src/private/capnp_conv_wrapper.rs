#[repr(transparent)]
pub struct CapnpConvWrapper<T> {
    wrapped_value: T,
}

impl<T> CapnpConvWrapper<T> {
    pub fn new(wrapped_value: T) -> Self {
        CapnpConvWrapper { wrapped_value }
    }

    pub fn into_wrapped_value(self) -> T {
        self.wrapped_value
    }
}
