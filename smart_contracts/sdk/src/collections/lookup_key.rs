use borsh::BorshSerialize;

pub trait LookupKey<'a>: Default {
    type Output: AsRef<[u8]> + 'a;
    fn lookup<T: BorshSerialize>(&self, prefix: &'a [u8], key: &T) -> Self::Output;
}

pub trait LookupKeyOwned: for<'a> LookupKey<'a> {}
impl<T> LookupKeyOwned for T where T: for<'a> LookupKey<'a> {}

#[derive(Default)]
pub struct Identity;
impl<'a> LookupKey<'a> for Identity {
    type Output = &'a [u8];

    #[inline(always)]
    fn lookup<T: BorshSerialize>(&self, prefix: &'a [u8], _key: &T) -> Self::Output {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_should_work() {
        let identity = Identity;
        let prefix = b"foo";
        let key = 123u64;
        assert_eq!(identity.lookup(prefix, &key), prefix);
    }
}
