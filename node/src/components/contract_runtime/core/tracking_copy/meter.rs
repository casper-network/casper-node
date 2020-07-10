/// Trait for measuring "size" of key-value pairs.
pub trait Meter<K, V> {
    fn measure(&self, k: &K, v: &V) -> usize;
}

pub mod heap_meter {
    use crate::components::contract_runtime::core::tracking_copy::byte_size::ByteSize;

    pub struct HeapSize;

    impl<K: ByteSize, V: ByteSize> super::Meter<K, V> for HeapSize {
        fn measure(&self, _: &K, v: &V) -> usize {
            std::mem::size_of::<V>() + v.byte_size()
        }
    }
}

#[cfg(test)]
pub mod count_meter {
    pub struct Count;

    impl<K, V> super::Meter<K, V> for Count {
        fn measure(&self, _k: &K, _v: &V) -> usize {
            1
        }
    }
}
