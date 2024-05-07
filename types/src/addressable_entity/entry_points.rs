use core::fmt::{Debug, Display, Formatter};

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::from_str;
#[cfg(feature = "json-schema")]
use serde_map_to_array::KeyValueJsonSchema;
use serde_map_to_array::{BTreeMapToArray, KeyValueLabels};

use crate::{
    addressable_entity::FromStrError,
    bytesrepr,
    bytesrepr::{Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex, CLType, EntityAddr, Group, HashAddr, BLAKE2B_DIGEST_LENGTH, KEY_HASH_LENGTH,
};

const V1_ENTRY_POINT_TAG: u8 = 0;
const V2_ENTRY_POINT_TAG: u8 = 1;

const V1_ENTRY_POINT_PREFIX: &str = "entry-point-v1-";
const V2_ENTRY_POINT_PREFIX: &str = "entry-point-v2-";

/// Context of method execution
///
/// Most significant bit represents version i.e.
/// - 0b0 -> 0.x/1.x (session & contracts)
/// - 0b1 -> 2.x and later (introduced installer, utility entry points)
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromPrimitive)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointType {
    /// Runs using the calling entity's context.
    /// In v1.x this was used for both "session" code run using the originating
    /// Account's context, and also for "StoredSession" code that ran in the
    /// caller's context. While this made systemic sense due to the way the runtime
    /// context nesting works, this dual usage was very confusing to most human beings.
    ///
    /// In v2.x the renamed Caller variant is exclusively used for wasm run using the initiating
    /// account entity's context. Previously installed 1.x stored session code should
    /// continue to work as the binary value matches but we no longer allow such logic
    /// to be upgraded, nor do we allow new stored session to be installed.
    Caller = 0b00000000,
    /// Runs using the called entity's context.
    Called = 0b00000001,
    /// Extract a subset of bytecode and installs it as a new smart contract.
    /// Runs using the called entity's context.
    Factory = 0b10000000,
}

impl EntryPointType {
    /// Checks if entry point type is introduced before 2.0.
    ///
    /// This method checks if there is a bit pattern for entry point types introduced in 2.0.
    ///
    /// If this bit is missing, that means given entry point type was defined in pre-2.0 world.
    pub fn is_legacy_pattern(&self) -> bool {
        (*self as u8) & 0b10000000 == 0
    }

    /// Get the bit pattern.
    pub fn bits(self) -> u8 {
        self as u8
    }

    /// Returns true if entry point type is invalid for the context.
    pub fn is_invalid_context(&self) -> bool {
        match self {
            EntryPointType::Caller => true,
            EntryPointType::Called | EntryPointType::Factory => false,
        }
    }
}

impl ToBytes for EntryPointType {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.bits().to_bytes()
    }

    fn serialized_length(&self) -> usize {
        1
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.bits());
        Ok(())
    }
}

impl FromBytes for EntryPointType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, bytes) = u8::from_bytes(bytes)?;
        let entry_point_type =
            EntryPointType::from_u8(value).ok_or(bytesrepr::Error::Formatting)?;
        Ok((entry_point_type, bytes))
    }
}

/// Default name for an entry point.
pub const DEFAULT_ENTRY_POINT_NAME: &str = "call";

/// Collection of entry point parameters.
pub type Parameters = Vec<Parameter>;

/// An enum specifying who pays for the invocation and execution of the entrypoint.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[repr(u8)]
pub enum EntryPointPayment {
    /// The caller must cover cost
    Caller = 0,
    /// Will cover cost to execute self but not cost of any subsequent invoked contracts
    SelfOnly = 1,
    /// will cover cost to execute self and the cost of any subsequent invoked contracts
    SelfOnward = 2,
}

impl ToBytes for EntryPointPayment {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.push(*self as u8);
        Ok(())
    }
}

impl FromBytes for EntryPointPayment {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (id, rem) = u8::from_bytes(bytes)?;
        let tag = match id {
            tag if tag == EntryPointPayment::Caller as u8 => EntryPointPayment::Caller,
            tag if tag == EntryPointPayment::SelfOnly as u8 => EntryPointPayment::SelfOnly,
            tag if tag == EntryPointPayment::SelfOnward as u8 => EntryPointPayment::SelfOnward,
            _ => return Err(Error::Formatting),
        };
        Ok((tag, rem))
    }
}

/// Type signature of a method. Order of arguments matter since can be
/// referenced by index as well as name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EntryPoint {
    name: String,
    args: Parameters,
    ret: CLType,
    access: EntryPointAccess,
    entry_point_type: EntryPointType,
    entry_point_payment: EntryPointPayment,
}

impl From<EntryPoint>
    for (
        String,
        Parameters,
        CLType,
        EntryPointAccess,
        EntryPointType,
        EntryPointPayment,
    )
{
    fn from(entry_point: EntryPoint) -> Self {
        (
            entry_point.name,
            entry_point.args,
            entry_point.ret,
            entry_point.access,
            entry_point.entry_point_type,
            entry_point.entry_point_payment,
        )
    }
}

impl EntryPoint {
    /// `EntryPoint` constructor.
    pub fn new<T: Into<String>>(
        name: T,
        args: Parameters,
        ret: CLType,
        access: EntryPointAccess,
        entry_point_type: EntryPointType,
        entry_point_payment: EntryPointPayment,
    ) -> Self {
        EntryPoint {
            name: name.into(),
            args,
            ret,
            access,
            entry_point_type,
            entry_point_payment,
        }
    }

    /// Create a default [`EntryPoint`] with specified name.
    pub fn default_with_name<T: Into<String>>(name: T) -> Self {
        EntryPoint {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Get name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get access enum.
    pub fn access(&self) -> &EntryPointAccess {
        &self.access
    }

    /// Get the arguments for this method.
    pub fn args(&self) -> &[Parameter] {
        self.args.as_slice()
    }

    /// Get the return type.
    pub fn ret(&self) -> &CLType {
        &self.ret
    }

    /// Obtains entry point
    pub fn entry_point_type(&self) -> EntryPointType {
        self.entry_point_type
    }
}

impl Default for EntryPoint {
    /// constructor for a public session `EntryPoint` that takes no args and returns `Unit`
    fn default() -> Self {
        EntryPoint {
            name: DEFAULT_ENTRY_POINT_NAME.to_string(),
            args: Vec::new(),
            ret: CLType::Unit,
            access: EntryPointAccess::Public,
            entry_point_type: EntryPointType::Caller,
            entry_point_payment: EntryPointPayment::Caller,
        }
    }
}

impl ToBytes for EntryPoint {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length()
            + self.args.serialized_length()
            + self.ret.serialized_length()
            + self.access.serialized_length()
            + self.entry_point_type.serialized_length()
            + self.entry_point_payment.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.name.write_bytes(writer)?;
        self.args.write_bytes(writer)?;
        self.ret.append_bytes(writer)?;
        self.access.write_bytes(writer)?;
        self.entry_point_type.write_bytes(writer)?;
        self.entry_point_payment.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for EntryPoint {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, bytes) = String::from_bytes(bytes)?;
        let (args, bytes) = Vec::<Parameter>::from_bytes(bytes)?;
        let (ret, bytes) = CLType::from_bytes(bytes)?;
        let (access, bytes) = EntryPointAccess::from_bytes(bytes)?;
        let (entry_point_type, bytes) = EntryPointType::from_bytes(bytes)?;
        let (entry_point_payment, bytes) = EntryPointPayment::from_bytes(bytes)?;

        Ok((
            EntryPoint {
                name,
                args,
                ret,
                access,
                entry_point_type,
                entry_point_payment,
            },
            bytes,
        ))
    }
}

/// Enum describing the possible access control options for a contract entry
/// point (method).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointAccess {
    /// Anyone can call this method (no access controls).
    Public,
    /// Only users from the listed groups may call this method. Note: if the
    /// list is empty then this method is not callable from outside the
    /// contract.
    Groups(Vec<Group>),
    /// Can't be accessed directly but are kept in the derived wasm bytes.
    Template,
}

const ENTRYPOINTACCESS_PUBLIC_TAG: u8 = 1;
const ENTRYPOINTACCESS_GROUPS_TAG: u8 = 2;
const ENTRYPOINTACCESS_ABSTRACT_TAG: u8 = 3;

impl EntryPointAccess {
    /// Constructor for access granted to only listed groups.
    pub fn groups(labels: &[&str]) -> Self {
        let list: Vec<Group> = labels
            .iter()
            .map(|s| Group::new(String::from(*s)))
            .collect();
        EntryPointAccess::Groups(list)
    }
}

impl ToBytes for EntryPointAccess {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;

        match self {
            EntryPointAccess::Public => {
                result.push(ENTRYPOINTACCESS_PUBLIC_TAG);
            }
            EntryPointAccess::Groups(groups) => {
                result.push(ENTRYPOINTACCESS_GROUPS_TAG);
                result.append(&mut groups.to_bytes()?);
            }
            EntryPointAccess::Template => {
                result.push(ENTRYPOINTACCESS_ABSTRACT_TAG);
            }
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        match self {
            EntryPointAccess::Public => 1,
            EntryPointAccess::Groups(groups) => 1 + groups.serialized_length(),
            EntryPointAccess::Template => 1,
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            EntryPointAccess::Public => {
                writer.push(ENTRYPOINTACCESS_PUBLIC_TAG);
            }
            EntryPointAccess::Groups(groups) => {
                writer.push(ENTRYPOINTACCESS_GROUPS_TAG);
                groups.write_bytes(writer)?;
            }
            EntryPointAccess::Template => {
                writer.push(ENTRYPOINTACCESS_ABSTRACT_TAG);
            }
        }
        Ok(())
    }
}

impl FromBytes for EntryPointAccess {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, bytes) = u8::from_bytes(bytes)?;

        match tag {
            ENTRYPOINTACCESS_PUBLIC_TAG => Ok((EntryPointAccess::Public, bytes)),
            ENTRYPOINTACCESS_GROUPS_TAG => {
                let (groups, bytes) = Vec::<Group>::from_bytes(bytes)?;
                let result = EntryPointAccess::Groups(groups);
                Ok((result, bytes))
            }
            ENTRYPOINTACCESS_ABSTRACT_TAG => Ok((EntryPointAccess::Template, bytes)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Parameter to a method
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Parameter {
    name: String,
    cl_type: CLType,
}

impl Parameter {
    /// `Parameter` constructor.
    pub fn new<T: Into<String>>(name: T, cl_type: CLType) -> Self {
        Parameter {
            name: name.into(),
            cl_type,
        }
    }

    /// Get the type of this argument.
    pub fn cl_type(&self) -> &CLType {
        &self.cl_type
    }

    /// Get a reference to the parameter's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl From<Parameter> for (String, CLType) {
    fn from(parameter: Parameter) -> Self {
        (parameter.name, parameter.cl_type)
    }
}

impl ToBytes for Parameter {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = ToBytes::to_bytes(&self.name)?;
        self.cl_type.append_bytes(&mut result)?;

        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.name) + self.cl_type.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.name.write_bytes(writer)?;
        self.cl_type.append_bytes(writer)
    }
}

impl FromBytes for Parameter {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, bytes) = String::from_bytes(bytes)?;
        let (cl_type, bytes) = CLType::from_bytes(bytes)?;

        Ok((Parameter { name, cl_type }, bytes))
    }
}

/// Collection of named entry points.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(transparent, deny_unknown_fields)]
pub struct EntryPoints(
    #[serde(with = "BTreeMapToArray::<String, EntryPoint, EntryPointLabels>")]
    BTreeMap<String, EntryPoint>,
);

impl ToBytes for EntryPoints {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for EntryPoints {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entry_points_map, remainder) = BTreeMap::<String, EntryPoint>::from_bytes(bytes)?;
        Ok((EntryPoints(entry_points_map), remainder))
    }
}

impl Default for EntryPoints {
    fn default() -> Self {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::default();
        entry_points.add_entry_point(entry_point);
        entry_points
    }
}

impl EntryPoints {
    /// Constructs a new, empty `EntryPoints`.
    pub const fn new() -> EntryPoints {
        EntryPoints(BTreeMap::<String, EntryPoint>::new())
    }

    /// Constructs a new `EntryPoints` with a single entry for the default `EntryPoint`.
    pub fn new_with_default_entry_point() -> Self {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::default();
        entry_points.add_entry_point(entry_point);
        entry_points
    }

    /// Adds new [`EntryPoint`].
    pub fn add_entry_point(&mut self, entry_point: EntryPoint) {
        self.0.insert(entry_point.name().to_string(), entry_point);
    }

    /// Checks if given [`EntryPoint`] exists.
    pub fn has_entry_point(&self, entry_point_name: &str) -> bool {
        self.0.contains_key(entry_point_name)
    }

    /// Gets an existing [`EntryPoint`] by its name.
    pub fn get(&self, entry_point_name: &str) -> Option<&EntryPoint> {
        self.0.get(entry_point_name)
    }

    /// Returns iterator for existing entry point names.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    /// Takes all entry points.
    pub fn take_entry_points(self) -> Vec<EntryPoint> {
        self.0.into_values().collect()
    }

    /// Returns the length of the entry points
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Checks if the `EntryPoints` is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Checks if any of the entry points are of the type Session.
    pub fn contains_stored_session(&self) -> bool {
        self.0
            .values()
            .any(|entry_point| entry_point.entry_point_type == EntryPointType::Caller)
    }
}

impl From<Vec<EntryPoint>> for EntryPoints {
    fn from(entry_points: Vec<EntryPoint>) -> EntryPoints {
        let entries = entry_points
            .into_iter()
            .map(|entry_point| (String::from(entry_point.name()), entry_point))
            .collect();
        EntryPoints(entries)
    }
}

struct EntryPointLabels;

impl KeyValueLabels for EntryPointLabels {
    const KEY: &'static str = "name";
    const VALUE: &'static str = "entry_point";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for EntryPointLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("NamedEntryPoint");
}

/// The entry point for the V2 Casper VM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EntryPointV2 {
    /// The selector.
    pub function_index: u32,
    /// The flags.
    pub flags: u32,
}

impl ToBytes for EntryPointV2 {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.function_index.serialized_length() + self.flags.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.append(&mut self.function_index.to_bytes()?);
        writer.append(&mut self.flags.to_bytes()?);
        Ok(())
    }
}

impl FromBytes for EntryPointV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (function_index, remainder) = u32::from_bytes(bytes)?;
        let (flags, remainder) = u32::from_bytes(remainder)?;
        Ok((
            EntryPointV2 {
                function_index,
                flags,
            },
            remainder,
        ))
    }
}

/// The entry point address.
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointAddr {
    /// The address for a V1 Entrypoint.
    VmCasperV1 {
        /// The addr of the entity.
        entity_addr: EntityAddr,
        /// The 32 byte hash of the name of the entry point
        name_bytes: [u8; KEY_HASH_LENGTH],
    },
    /// The address for a V2 entrypoint
    VmCasperV2 {
        /// The addr of the entity.
        entity_addr: EntityAddr,
        /// The selector.
        selector: u32,
    },
}

impl EntryPointAddr {
    /// Returns a `VmCasperV1` variant of the entry point address.
    pub fn new_v1_entry_point_addr(
        entity_addr: EntityAddr,
        name: &str,
    ) -> Result<Self, bytesrepr::Error> {
        let bytes = name.to_bytes()?;
        let mut hasher = {
            match VarBlake2b::new(BLAKE2B_DIGEST_LENGTH) {
                Ok(hasher) => hasher,
                Err(_) => return Err(bytesrepr::Error::Formatting),
            }
        };
        hasher.update(bytes);
        // NOTE: Assumed safe as size of `HashAddr` equals to the output provided by hasher.
        let mut name_bytes = HashAddr::default();
        hasher.finalize_variable(|hash| name_bytes.clone_from_slice(hash));
        Ok(Self::VmCasperV1 {
            entity_addr,
            name_bytes,
        })
    }

    /// Returns a `VmCasperV2` variant of the entry point address.
    pub const fn new_v2_entry_point_addr(entity_addr: EntityAddr, selector: u32) -> Self {
        Self::VmCasperV2 {
            entity_addr,
            selector,
        }
    }

    /// Returns the encapsulated [`EntityAddr`].
    pub fn entity_addr(&self) -> EntityAddr {
        match self {
            EntryPointAddr::VmCasperV1 { entity_addr, .. } => *entity_addr,
            EntryPointAddr::VmCasperV2 { entity_addr, .. } => *entity_addr,
        }
    }

    /// Returns the formatted String representation of the [`EntryPointAddr`].
    pub fn to_formatted_string(&self) -> String {
        format!("{}", self)
    }

    /// Returns the address from the formatted string.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        if let Some(entry_point_v1) = input.strip_prefix(V1_ENTRY_POINT_PREFIX) {
            if let Some((entity_addr_str, string_bytes_str)) = entry_point_v1.rsplit_once('-') {
                let entity_addr = EntityAddr::from_formatted_str(entity_addr_str)?;
                let string_bytes =
                    checksummed_hex::decode(string_bytes_str).map_err(FromStrError::Hex)?;
                let (name_bytes, _) =
                    FromBytes::from_vec(string_bytes).map_err(FromStrError::BytesRepr)?;
                return Ok(Self::VmCasperV1 {
                    entity_addr,
                    name_bytes,
                });
            }
        }

        if let Some(entry_point_v2) = input.strip_prefix(V2_ENTRY_POINT_PREFIX) {
            if let Some((entity_addr_str, selector_str)) = entry_point_v2.rsplit_once('-') {
                let entity_addr = EntityAddr::from_formatted_str(entity_addr_str)?;
                let selector: u32 = from_str(selector_str)
                    .map_err(|_| FromStrError::BytesRepr(Error::Formatting))?;
                return Ok(Self::VmCasperV2 {
                    entity_addr,
                    selector,
                });
            }
        }

        Err(FromStrError::InvalidPrefix)
    }
}

impl ToBytes for EntryPointAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            EntryPointAddr::VmCasperV1 {
                entity_addr,
                name_bytes: named_bytes,
            } => {
                buffer.insert(0, V1_ENTRY_POINT_TAG);
                buffer.append(&mut entity_addr.to_bytes()?);
                buffer.append(&mut named_bytes.to_bytes()?);
            }
            EntryPointAddr::VmCasperV2 {
                entity_addr,
                selector,
            } => {
                buffer.insert(0, V2_ENTRY_POINT_TAG);
                buffer.append(&mut entity_addr.to_bytes()?);
                buffer.append(&mut selector.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                EntryPointAddr::VmCasperV1 {
                    entity_addr,
                    name_bytes: named_bytes,
                } => entity_addr.serialized_length() + named_bytes.serialized_length(),
                EntryPointAddr::VmCasperV2 {
                    entity_addr,
                    selector,
                } => entity_addr.serialized_length() + selector.serialized_length(),
            }
    }
}

impl FromBytes for EntryPointAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, bytes) = u8::from_bytes(bytes)?;
        match tag {
            V1_ENTRY_POINT_TAG => {
                let (entity_addr, bytes) = EntityAddr::from_bytes(bytes)?;
                let (name_bytes, bytes) = FromBytes::from_bytes(bytes)?;
                Ok((
                    Self::VmCasperV1 {
                        entity_addr,
                        name_bytes,
                    },
                    bytes,
                ))
            }
            V2_ENTRY_POINT_TAG => {
                let (entity_addr, bytes) = EntityAddr::from_bytes(bytes)?;
                let (selector, bytes) = u32::from_bytes(bytes)?;
                Ok((
                    Self::VmCasperV2 {
                        entity_addr,
                        selector,
                    },
                    bytes,
                ))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl Display for EntryPointAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            EntryPointAddr::VmCasperV1 {
                entity_addr,
                name_bytes,
            } => {
                write!(
                    f,
                    "{}{}-{}",
                    V1_ENTRY_POINT_PREFIX,
                    entity_addr,
                    base16::encode_lower(name_bytes)
                )
            }
            EntryPointAddr::VmCasperV2 {
                entity_addr,
                selector,
            } => {
                write!(f, "{}{}-{}", V2_ENTRY_POINT_PREFIX, entity_addr, selector)
            }
        }
    }
}

impl Debug for EntryPointAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            EntryPointAddr::VmCasperV1 {
                entity_addr,
                name_bytes,
            } => {
                write!(f, "EntryPointAddr({:?}-{:?})", entity_addr, name_bytes)
            }
            EntryPointAddr::VmCasperV2 {
                entity_addr,
                selector,
            } => {
                write!(f, "EntryPointAddr({:?}-{:?})", entity_addr, selector)
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<EntryPointAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntryPointAddr {
        if rng.gen() {
            EntryPointAddr::VmCasperV1 {
                entity_addr: rng.gen(),
                name_bytes: rng.gen(),
            }
        } else {
            EntryPointAddr::VmCasperV2 {
                entity_addr: rng.gen(),
                selector: rng.gen(),
            }
        }
    }
}

/// The encaspulated representation of entrypoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointValue {
    /// Entrypoints to be executed against the V1 Casper VM.
    V1CasperVm(EntryPoint),
    /// Entrypoints to be executed against the V2 Casper VM.
    V2CasperVm(EntryPointV2),
}

impl EntryPointValue {
    /// Returns [`EntryPointValue::V1CasperVm`] variant.
    pub fn new_v1_entry_point_value(entry_point: EntryPoint) -> Self {
        Self::V1CasperVm(entry_point)
    }

    /// Returns [`EntryPointValue::V2CasperVm`] variant.
    pub fn new_v2_entry_point_value(entry_point: EntryPointV2) -> Self {
        Self::V2CasperVm(entry_point)
    }
}

impl ToBytes for EntryPointValue {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                EntryPointValue::V1CasperVm(entry_point) => entry_point.serialized_length(),
                EntryPointValue::V2CasperVm(entry_point) => entry_point.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            EntryPointValue::V1CasperVm(entry_point) => {
                writer.push(V1_ENTRY_POINT_TAG);
                entry_point.write_bytes(writer)?;
            }
            EntryPointValue::V2CasperVm(entry_point) => {
                writer.push(V2_ENTRY_POINT_TAG);
                entry_point.write_bytes(writer)?;
            }
        }
        Ok(())
    }
}

impl FromBytes for EntryPointValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            V1_ENTRY_POINT_TAG => {
                let (entry_point, remainder) = EntryPoint::from_bytes(remainder)?;
                Ok((Self::V1CasperVm(entry_point), remainder))
            }
            V2_ENTRY_POINT_TAG => {
                let (entry_point, remainder) = EntryPointV2::from_bytes(remainder)?;
                Ok((Self::V2CasperVm(entry_point), remainder))
            }
            _ => Err(Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_point_type_serialization_roundtrip() {
        let vm1 = EntryPointAddr::VmCasperV1 {
            entity_addr: EntityAddr::new_smart_contract([42; 32]),
            name_bytes: [99; 32],
        };
        bytesrepr::test_serialization_roundtrip(&vm1);
        let vm2 = EntryPointAddr::VmCasperV2 {
            entity_addr: EntityAddr::new_smart_contract([42; 32]),
            selector: u32::MAX,
        };
        bytesrepr::test_serialization_roundtrip(&vm2);
    }
}
