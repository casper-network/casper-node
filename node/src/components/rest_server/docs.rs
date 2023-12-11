use casper_types::{ProtocolVersion, PublicKey, SecretKey, Timestamp};
use once_cell::sync::Lazy;

use crate::types::InternalEraReport;

pub(crate) const DOCS_EXAMPLE_PROTOCOL_VERSION: ProtocolVersion =
    ProtocolVersion::from_parts(1, 5, 3);

/// A trait used to generate a static hardcoded example of `Self`.
pub trait DocExample {
    /// Generates a hardcoded example of `Self`.
    fn doc_example() -> &'static Self;
}

impl DocExample for Timestamp {
    fn doc_example() -> &'static Self {
        Timestamp::example()
    }
}

static INTERNAL_ERA_REPORT: Lazy<InternalEraReport> = Lazy::new(|| {
    let secret_key_1 = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key_1 = PublicKey::from(&secret_key_1);
    let equivocators = vec![public_key_1];

    let secret_key_3 = SecretKey::ed25519_from_bytes([2; 32]).unwrap();
    let public_key_3 = PublicKey::from(&secret_key_3);
    let inactive_validators = vec![public_key_3];

    InternalEraReport {
        equivocators,
        inactive_validators,
    }
});

impl DocExample for InternalEraReport {
    fn doc_example() -> &'static Self {
        &INTERNAL_ERA_REPORT
    }
}
