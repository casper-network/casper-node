use std::{cell::RefCell, collections::BTreeMap, ptr};

use bytes::Bytes;

use super::{Address, CreateResult, Entry, Error, Manifest};

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
struct TaggedValue {
    tag: u64,
    value: Bytes,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
struct BorrowedTaggedValue<'a> {
    tag: u64,
    value: &'a [u8],
}
type Container = BTreeMap<u64, BTreeMap<Bytes, TaggedValue>>;

#[derive(Default, Clone)]
pub(crate) struct LocalKV {
    db: Container,
}

// impl LocalKV {
//     pub(crate) fn update(&mut self, db: LocalKV) {
//         self.db = db.db
//     }
// }

thread_local! {
    static DB: RefCell<LocalKV> = RefCell::new(LocalKV::default());
}

pub fn copy_input() -> Vec<u8> {
    todo!()
}

pub fn print(msg: &str) {
    println!("💻 {msg}");
}
pub fn write(key_space: u64, key: &[u8], value_tag: u64, value: &[u8]) -> Result<(), Error> {
    DB.with(|db| {
        db.borrow_mut().db.entry(key_space).or_default().insert(
            Bytes::copy_from_slice(key),
            TaggedValue {
                tag: value_tag,
                value: Bytes::copy_from_slice(value),
            },
        );
    });
    Ok(())
}
pub fn read(
    key_space: u64,
    key: &[u8],
    func: impl FnOnce(usize) -> Option<ptr::NonNull<u8>>,
) -> Result<Option<Entry>, Error> {
    let value = DB.with(|db| db.borrow().db.get(&key_space)?.get(key).cloned());
    match value {
        Some(tagged_value) => {
            let entry = Entry {
                tag: tagged_value.tag,
            };

            let ptr = func(tagged_value.value.len());

            if let Some(ptr) = ptr {
                unsafe {
                    ptr::copy_nonoverlapping(
                        tagged_value.value.as_ptr(),
                        ptr.as_ptr(),
                        tagged_value.value.len(),
                    );
                }
            }

            Ok(Some(entry))
        }
        None => Ok(None),
    }
}

// pub fn dispatch<Args, R>(export: impl Fn(Args) -> R, args: Args) -> R {
//     export(args)
// }
pub fn revert(code: u32) -> ! {
    panic!("revert with code {code}")
}

pub fn create(_code: Option<&[u8]>, _manifest: &Manifest) -> Result<CreateResult, Error> {
    todo!()
}

pub fn call(
    address: &Address,
    value: u64,
    entry_point: &str,
    input_data: &[u8],
) -> Result<Vec<u8>, Error> {
    todo!()
}
