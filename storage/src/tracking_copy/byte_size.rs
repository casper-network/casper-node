use std::mem;

use casper_types::{account::Account, bytesrepr::ToBytes, ByteCode, Key, StoredValue};

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

impl ByteSize for StoredValue {
    fn byte_size(&self) -> usize {
        mem::size_of::<Self>()
            + match self {
                StoredValue::CLValue(cl_value) => cl_value.serialized_length(),
                StoredValue::Account(account) => account.serialized_length(),
                StoredValue::ContractWasm(contract_wasm) => contract_wasm.serialized_length(),
                StoredValue::ContractPackage(contract_package) => {
                    contract_package.serialized_length()
                }
                StoredValue::Contract(contract) => contract.serialized_length(),
                StoredValue::AddressableEntity(contract_header) => {
                    contract_header.serialized_length()
                }
                StoredValue::Package(package) => package.serialized_length(),
                StoredValue::DeployInfo(deploy_info) => deploy_info.serialized_length(),
                StoredValue::Transfer(transfer) => transfer.serialized_length(),
                StoredValue::EraInfo(era_info) => era_info.serialized_length(),
                StoredValue::Bid(bid) => bid.serialized_length(),
                StoredValue::BidKind(bid_kind) => bid_kind.serialized_length(),
                StoredValue::Withdraw(withdraw_purses) => withdraw_purses.serialized_length(),
                StoredValue::Unbonding(unbonding_purses) => unbonding_purses.serialized_length(),
                StoredValue::ByteCode(byte_code) => byte_code.serialized_length(),
                StoredValue::MessageTopic(message_topic_summary) => {
                    message_topic_summary.serialized_length()
                }
                StoredValue::Message(message_summary) => message_summary.serialized_length(),
                StoredValue::NamedKey(named_key) => named_key.serialized_length(),
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
        // NOTE: We're ignoring size of the tree's nodes.
        self.named_keys()
            .iter()
            .fold(0, |sum, (k, v)| sum + k.heap_size() + v.heap_size())
    }
}

// TODO: contract has other fields (re protocol version) that are not repr here...on purpose?
impl HeapSizeOf for ByteCode {
    fn heap_size(&self) -> usize {
        self.bytes().len()
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
    use std::mem;

    use super::ByteSize;

    fn assert_byte_size<T: ByteSize>(el: T, expected: usize) {
        assert_eq!(el.byte_size(), expected)
    }

    #[test]
    fn byte_size_of_string() {
        assert_byte_size("Hello".to_owned(), 5 + mem::size_of::<String>())
    }
}
