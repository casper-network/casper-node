use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "std")] {
        pub use ::std::{format, borrow, string, vec, boxed, fmt, str, marker, ffi, ptr, mem, cmp};

        pub mod collections {
            pub use ::std::collections::btree_map::{self, BTreeMap};
            pub use ::std::collections::{linked_list::{self, LinkedList}};
            pub use ::std::collections::{hash_map::{self, HashMap}};
            pub use ::std::collections::{btree_set::{self, BTreeSet}};
        }
    }
    else {
        pub use ::alloc::{format, borrow, string, vec, boxed, fmt, str};

        pub use ::core::{marker, ffi, ptr, mem, cmp};

        pub mod collections {
            pub use ::alloc::collections::btree_map::{self, BTreeMap};
            pub use ::alloc::collections::{linked_list::{self, LinkedList}};
            #[cfg(feature = "hashbrown")]
            pub use ::alloc::collections::{hash_map::{self, HashMap}};
            pub use ::alloc::collections::{btree_set::{self, BTreeSet}};
        }
    }
}

pub use self::{
    borrow::ToOwned,
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};

pub use crate::{
    casper::{self, Entity},
    log,
    macros::{self, casper, PanicOnDefault},
    ret, revert, rollback,
};

#[cfg(test)]
mod tests {

    #[test]
    fn test_format() {
        assert_eq!(super::format!("Hello, {}!", "world"), "Hello, world!");
    }

    #[test]
    fn test_string() {
        let s = super::String::from("hello");
        assert_eq!(s, "hello");
    }

    #[test]
    #[allow(clippy::vec_init_then_push)]
    fn test_vec() {
        let mut v = super::Vec::new();
        v.push(1);
        v.push(2);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], 1);
        assert_eq!(v[1], 2);
    }
}
