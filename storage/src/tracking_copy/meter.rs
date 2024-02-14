use std::collections::BTreeSet;

/// Trait for measuring "size" of key-value pairs.
pub trait Meter<K, V> {
    fn measure(&self, k: &K, v: &V) -> usize;

    fn measure_keys(&self, keys: &BTreeSet<K>) -> usize;
}

pub mod heap_meter {
    use std::collections::BTreeSet;

    use crate::tracking_copy::byte_size::ByteSize;

    pub struct HeapSize;

    impl<K: ByteSize, V: ByteSize> super::Meter<K, V> for HeapSize {
        fn measure(&self, _: &K, v: &V) -> usize {
            std::mem::size_of::<V>() + v.byte_size()
        }

        fn measure_keys(&self, keys: &BTreeSet<K>) -> usize {
            let mut total: usize = 0;
            for key in keys {
                total += key.byte_size();
            }
            total
        }
    }
}

#[cfg(test)]
pub mod count_meter {
    use std::collections::BTreeSet;

    pub struct Count;

    impl<K, V> super::Meter<K, V> for Count {
        fn measure(&self, _k: &K, _v: &V) -> usize {
            1
        }

        fn measure_keys(&self, _keys: &BTreeSet<K>) -> usize {
            unimplemented!()
        }
    }
}
