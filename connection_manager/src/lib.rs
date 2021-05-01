pub mod outgoing;

use std::{
    error,
    fmt::{self, Display, Formatter},
};

use tracing::field;

// Placeholders/copied from main source all below. No need to read further.

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Sha512([u8; 64]);

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyFingerprint(Sha512);

/// The network identifier for a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeId {
    Tls(KeyFingerprint),
}

pub type NodeRng = ChaCha20Rng;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

pub fn new_rng() -> NodeRng {
    NodeRng::from_entropy()
}

impl NodeId {
    /// Generates a random Tls instance using a `TestRng`.
    #[cfg(test)]
    pub(crate) fn random_tls(rng: &mut TestRng) -> Self {
        NodeId::Tls(rng.gen())
    }
}

impl Display for NodeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

pub(crate) fn display_error<'a, T>(err: &'a T) -> field::DisplayValue<ErrFormatter<'a, T>>
where
    T: error::Error + 'a,
{
    field::display(ErrFormatter(err))
}

/// An error formatter.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ErrFormatter<'a, T>(pub &'a T);

impl<'a, T> Display for ErrFormatter<'a, T>
where
    T: error::Error,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut opt_source: Option<&(dyn error::Error)> = Some(self.0);

        while let Some(source) = opt_source {
            write!(f, "{}", source)?;
            opt_source = source.source();

            if opt_source.is_some() {
                f.write_str(": ")?;
            }
        }

        Ok(())
    }
}
