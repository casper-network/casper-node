pub use ::borsh;

/// Input/output serialization conventions for the SDK.
#[derive(Default, PartialEq, Eq, Debug, Clone, Copy)]
pub enum Convention {
    /// Treats input bytes as a concatenated sequence of positional arguments.
    ///
    /// I.e. `borsh::to_vec(&(arg1, arg2, arg3))`.
    #[default]
    Positional,
    /// Treats VM1 `RuntimeArgs` compatible named arguments.
    ///
    /// This expects input bytes to be passed as a serialized `BTreeMap<String, CLValue>` or
    /// `Vec<(String, CLValue)>`.
    ///
    /// Each argument will be dispatched by its name.
    Named,
}

pub trait AbiConvention {
    const DEFAULT_ABI_CONVENTION: Convention;
}
