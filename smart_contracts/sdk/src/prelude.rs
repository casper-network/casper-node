use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "std")] {
        pub use ::std::{format, borrow, string, vec, boxed, fmt, str, marker, ffi, ptr, mem, cmp};
        // pub use ::std::collections::{btree_map}
        pub mod collections {
            pub use ::std::collections::btree_map::{self, BTreeMap};
        }
    }
    else {
        pub use ::alloc::{format, borrow, string, vec, boxed, fmt, str};

        pub use ::core::{marker, ffi, ptr, mem, cmp};

        pub mod collections {
            pub use ::alloc::collections::btree_map::{self, BTreeMap};
        }
    }
}

// pub use self::format::format;
pub use self::{
    borrow::ToOwned,
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
// pub use self::collections::btree_map::{self}

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
