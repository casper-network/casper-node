pub mod for_each_host_function;

/// Signature of a function pointer that a host understands.
pub type Fptr = extern "C" fn() -> ();

#[derive(Debug)]
#[repr(C, packed)]
pub struct ReadInfo {
    pub data_ptr: *const u8,
    /// Size in bytes
    pub data_size: usize,
}

#[repr(C, packed)]
#[derive(Debug)]
pub struct CreateResult {
    pub contract_address: [u8; 32],
}

#[repr(C, packed)]
#[derive(Debug)]
pub struct UpgradeResult {
    pub package_address: [u8; 32],
    pub contract_address: [u8; 32],
    pub version: u32,
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct EnvInfo {
    pub block_time: u64,
    pub transferred_value: u64,
    pub caller_addr: [u8; 32],
    pub caller_kind: u32,
    pub callee_addr: [u8; 32],
    pub callee_kind: u32,
    pub protocol_version_major: u32,
    pub protocol_version_minor: u32,
    pub protocol_version_patch: u32,
    pub parent_block_hash: [u8; 32],
    pub block_height: u64,
}

macro_rules! visit_host_function {
    ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty $(,)?)* ))? $(-> $ret:ty)?;)+) => {
        $(
            $(#[$cfg])? $vis fn $name($($($arg: $argty,)*)?) $(-> $ret)?;
        )*
    }
}

extern "C" {
    for_each_host_function!(visit_host_function);
}

macro_rules! visit_host_function_name {
    ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty $(,)?)* ))? $(-> $ret:ty)?;)+) => {
        &[
            $(
                stringify!($name),
            )*
        ]
    }
}

pub const HOST_FUNCTIONS: &[&str] = for_each_host_function!(visit_host_function_name);

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::HOST_FUNCTIONS;

    mod separate_module {
        use crate::for_each_host_function;

        macro_rules! visit_host_function {
            ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty $(,)?)* ))? $(-> $ret:ty)?;)+) => {
                $(
                    #[allow(dead_code, unused_variables, clippy::too_many_arguments)]
                    $(#[$cfg])? $vis fn $name($($($arg: $argty,)*)?) $(-> $ret)? {
                        todo!("Called fn {}", stringify!($name));
                    }
                )*
            }
        }
        for_each_host_function!(visit_host_function);
    }

    #[test]
    #[should_panic(expected = "Called fn casper_print")]
    fn different_module() {
        const MSG: &str = "foobar";
        separate_module::casper_print(MSG.as_ptr(), MSG.len());
    }

    #[test]
    fn all_host_functions() {
        let host_functions = BTreeSet::from_iter(HOST_FUNCTIONS);
        assert!(host_functions.contains(&"casper_call"));
    }
}
