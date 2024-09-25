use crate::serializers::borsh::{BorshDeserialize, BorshSerialize};

use crate::abi::CasperABI;

use super::Vector;

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct SortedVector<T: Ord> {
    vector: Vector<T>,
}

impl<T: Ord + CasperABI> CasperABI for SortedVector<T> {
    fn populate_definitions(definitions: &mut crate::abi::Definitions) {
        T::populate_definitions(definitions)
    }

    fn declaration() -> crate::abi::Declaration {
        format!("SortedVector<{}>", T::declaration())
    }

    fn definition() -> crate::abi::Definition {
        crate::abi::Definition::Struct {
            items: vec![
                crate::abi::StructField {
                    name: "prefix".into(),
                    decl: String::declaration(),
                },
                crate::abi::StructField {
                    name: "length".into(),
                    decl: u64::declaration(),
                },
            ],
        }
    }
}

impl<T> SortedVector<T>
where
    T: BorshSerialize + BorshDeserialize + Ord,
{
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        Self {
            vector: Vector::new(prefix),
        }
    }

    pub fn push(&mut self, value: T) {
        let pos = self.vector.binary_search(&value).unwrap_or_else(|e| e);
        self.vector.insert(pos, value);
    }

    #[inline]
    pub fn contains(&self, value: &T) -> bool {
        self.vector.binary_search(value).is_ok()
    }

    #[inline(always)]
    pub fn get(&self, index: u64) -> Option<T> {
        self.vector.get(index)
    }

    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.vector.iter()
    }

    #[inline(always)]
    pub fn len(&self) -> u64 {
        self.vector.len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.vector.is_empty()
    }

    #[inline(always)]
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.vector.retain(f)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use crate::host::native::dispatch;

    use super::*;

    #[test]
    fn test_sorted_vector() {
        dispatch(|| {
            let mut sorted_vector = SortedVector::new("sorted_vector");

            sorted_vector.push(2);
            sorted_vector.push(1);
            sorted_vector.push(3);
            sorted_vector.push(0);
            sorted_vector.push(0);
            sorted_vector.push(3);

            assert!(sorted_vector.contains(&0));
            assert!(sorted_vector.contains(&2));
            assert!(!sorted_vector.contains(&15));

            let vec: Vec<_> = sorted_vector.iter().collect();
            assert_eq!(vec, vec![0, 0, 1, 2, 3, 3]);
        })
        .unwrap();
    }
}
