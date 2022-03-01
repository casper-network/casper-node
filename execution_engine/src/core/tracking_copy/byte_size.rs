use std::{collections::BTreeMap, mem};

use casper_types::{account::Account, bytesrepr::ToBytes, ContractWasm, Key, StoredValue};

/// Returns byte size of the element - both heap size and stack size.
pub trait ByteSize {
    fn byte_size(&self) -> usize;
}

impl ByteSize for Key {
    fn byte_size(&self) -> usize {
        mem::size_of::<Self>() + self.heap_size()
    }
}

impl ByteSize for String {
    fn byte_size(&self) -> usize {
        mem::size_of::<Self>() + self.heap_size()
    }
}

impl<K: HeapSizeOf, V: HeapSizeOf> ByteSize for BTreeMap<K, V> {
    fn byte_size(&self) -> usize {
        mem::size_of::<BTreeMap<K, V>>()
            + self.heap_size()
            + self.len() * (mem::size_of::<K>() + mem::size_of::<V>())
    }
}

impl ByteSize for StoredValue {
    fn byte_size(&self) -> usize {
        mem::size_of::<Self>()
            + match self {
                StoredValue::CLValue(cl_value) => cl_value.serialized_length(),
                StoredValue::Account(account) => account.serialized_length(),
                StoredValue::ContractWasm(contract_wasm) => contract_wasm.serialized_length(),
                StoredValue::Contract(contract_header) => contract_header.serialized_length(),
                StoredValue::ContractPackage(contract_package) => {
                    contract_package.serialized_length()
                }
                StoredValue::DeployInfo(deploy_info) => deploy_info.serialized_length(),
                StoredValue::Transfer(transfer) => transfer.serialized_length(),
                StoredValue::EraInfo(era_info) => era_info.serialized_length(),
                StoredValue::Bid(bid) => bid.serialized_length(),
                StoredValue::Withdraw(withdraw_purses) => withdraw_purses.serialized_length(),
                StoredValue::Unbonding(unbonding_purses) => unbonding_purses.serialized_length(),
            }
    }
}

/// Returns heap size of the value.
/// Note it's different from [ByteSize] that returns both heap and stack size.
pub trait HeapSizeOf {
    fn heap_size(&self) -> usize;
}

impl HeapSizeOf for Key {
    fn heap_size(&self) -> usize {
        0
    }
}

// TODO: contract has other fields (re a bunch) that are not repr here...on purpose?
impl HeapSizeOf for Account {
    fn heap_size(&self) -> usize {
        self.named_keys().heap_size()
    }
}

// TODO: contract has other fields (re protocol version) that are not repr here...on purpose?
impl HeapSizeOf for ContractWasm {
    fn heap_size(&self) -> usize {
        self.bytes().len()
    }
}

// NOTE: We're ignoring size of the tree's nodes.
impl<K: HeapSizeOf, V: HeapSizeOf> HeapSizeOf for BTreeMap<K, V> {
    fn heap_size(&self) -> usize {
        self.iter()
            .fold(0, |sum, (k, v)| sum + k.heap_size() + v.heap_size())
    }
}

impl<T: HeapSizeOf> ByteSize for [T] {
    fn byte_size(&self) -> usize {
        self.iter()
            .fold(0, |sum, el| sum + mem::size_of::<T>() + el.heap_size())
    }
}

impl HeapSizeOf for String {
    fn heap_size(&self) -> usize {
        self.capacity()
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, mem};

    use super::ByteSize;
    use casper_types::Key;

    fn assert_byte_size<T: ByteSize>(el: T, expected: usize) {
        assert_eq!(el.byte_size(), expected)
    }

    #[test]
    fn byte_size_of_string() {
        assert_byte_size("Hello".to_owned(), 5 + mem::size_of::<String>())
    }

    #[test]
    fn byte_size_of_map() {
        let v = vec![
            (Key::Hash([1u8; 32]), "A".to_string()),
            (Key::Hash([2u8; 32]), "B".to_string()),
            (Key::Hash([3u8; 32]), "C".to_string()),
            (Key::Hash([4u8; 32]), "D".to_string()),
        ];
        let it_size: usize = mem::size_of::<BTreeMap<Key, String>>()
            + 4 * (mem::size_of::<Key>() + mem::size_of::<String>() + 1);
        let map: BTreeMap<Key, String> = v.into_iter().collect();
        assert_byte_size(map, it_size);
    }
}
