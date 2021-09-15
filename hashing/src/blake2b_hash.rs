use crate::Digest;

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(
    feature = "std",
    derive(schemars::JsonSchema, serde::Serialize, serde::Deserialize,),
    serde(deny_unknown_fields)
)]
pub(super) struct Blake2bHash(pub(crate) [u8; Digest::LENGTH]);

impl AsRef<[u8]> for Blake2bHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
