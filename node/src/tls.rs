//! Transport layer security and signing based on OpenSSL.
//!
//! This module wraps some of the lower-level TLS constructs to provide a reasonably safe-to-use API
//! surface for the rest of the application. It also fixes the security parameters of the TLS level
//! in a central place.
//!
//! Features include
//!
//! * a fixed set of chosen encryption parameters
//!   ([`SIGNATURE_ALGORITHM`](constant.SIGNATURE_ALGORITHM.html),
//!   [`SIGNATURE_CURVE`](constant.SIGNATURE_CURVE.html),
//!   [`SIGNATURE_DIGEST`](constant.SIGNATURE_DIGEST.html)),
//! * construction of TLS acceptors for listening TCP sockets
//!   ([`create_tls_acceptor`](fn.create_tls_acceptor.html)),
//! * construction of TLS connectors for outgoing TCP connections
//!   ([`create_tls_connector`](fn.create_tls_connector.html)),
//! * creation and validation of self-signed certificates
//!   ([`generate_node_cert`](fn.generate_node_cert.html)),
//! * signing and verification of arbitrary values using keys from certificates
//!   ([`Signature`](struct.Signature.html), [`Signed`](struct.Signed.html)), and
//! * `serde` support for certificates ([`x509_serde`](x509_serde/index.html))

use std::{
    cmp::Ordering,
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
    marker::PhantomData,
    path::Path,
    str,
    time::{SystemTime, UNIX_EPOCH},
};

use casper_types::file_utils::{read_file, ReadFileError};
use datasize::DataSize;
use hex_fmt::HexFmt;
use nid::Nid;
use openssl::{
    asn1::{Asn1Integer, Asn1IntegerRef, Asn1Time},
    bn::{BigNum, BigNumContext},
    ec::{self, EcKey},
    error::ErrorStack,
    hash::{DigestBytes, MessageDigest},
    nid,
    pkey::{PKey, PKeyRef, Private, Public},
    sha,
    ssl::{SslAcceptor, SslConnector, SslContextBuilder, SslMethod, SslVerifyMode, SslVersion},
    x509::{X509Builder, X509Name, X509NameBuilder, X509NameRef, X509Ref, X509},
};
#[cfg(test)]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::utils::LockedLineWriter;

// This is inside a private module so that the generated `BigArray` does not form part of this
// crate's public API, and hence also doesn't appear in the rustdocs.
mod big_array {
    use serde_big_array::big_array;

    big_array! { BigArray; }
}

/// The chosen signature algorithm (**ECDSA  with SHA512**).
const SIGNATURE_ALGORITHM: Nid = Nid::ECDSA_WITH_SHA512;

/// The underlying elliptic curve (**P-521**).
const SIGNATURE_CURVE: Nid = Nid::SECP521R1;

/// The chosen signature algorithm (**SHA512**).
const SIGNATURE_DIGEST: Nid = Nid::SHA512;

/// OpenSSL result type alias.
///
/// Many functions rely solely on `openssl` functions and return this kind of result.
type SslResult<T> = Result<T, ErrorStack>;

/// SHA512 hash.
#[derive(Copy, Clone, DataSize, Deserialize, Serialize)]
pub struct Sha512(#[serde(with = "big_array::BigArray")] [u8; Sha512::SIZE]);

impl Sha512 {
    /// Size of digest in bytes.
    const SIZE: usize = 64;

    /// OpenSSL NID.
    const NID: Nid = Nid::SHA512;

    /// Create a new Sha512 by hashing a slice.
    pub fn new<B: AsRef<[u8]>>(data: B) -> Self {
        let mut openssl_sha = sha::Sha512::new();
        openssl_sha.update(data.as_ref());
        Sha512(openssl_sha.finish())
    }

    /// Returns bytestring of the hash, with length `Self::SIZE`.
    fn bytes(&self) -> &[u8] {
        let bs = &self.0[..];

        debug_assert_eq!(bs.len(), Self::SIZE);
        bs
    }

    /// Converts an OpenSSL digest into an `Sha512`.
    fn from_openssl_digest(digest: &DigestBytes) -> Self {
        let digest_bytes = digest.as_ref();

        debug_assert_eq!(
            digest_bytes.len(),
            Self::SIZE,
            "digest is not the right size - check constants in `tls.rs`"
        );

        let mut buf = [0; Self::SIZE];
        buf.copy_from_slice(&digest_bytes[0..Self::SIZE]);

        Sha512(buf)
    }

    /// Returns a new OpenSSL `MessageDigest` set to SHA-512.
    fn create_message_digest() -> MessageDigest {
        // This can only fail if we specify a `Nid` that does not exist, which cannot happen unless
        // there is something wrong with `Self::NID`.
        MessageDigest::from_nid(Self::NID).expect("Sha512::NID is invalid")
    }
}

/// Certificate fingerprint.
#[derive(Copy, Clone, DataSize, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct CertFingerprint(Sha512);

impl Debug for CertFingerprint {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "CertFingerprint({:10})", HexFmt(self.0.bytes()))
    }
}

/// Public key fingerprint.
#[derive(Copy, Clone, DataSize, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KeyFingerprint(Sha512);

impl KeyFingerprint {
    /// Size of digest in bytes.
    pub const LENGTH: usize = Sha512::SIZE;
}

impl AsRef<[u8]> for KeyFingerprint {
    fn as_ref(&self) -> &[u8] {
        self.0.bytes()
    }
}

impl Debug for KeyFingerprint {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "KeyFingerprint({:10})", HexFmt(self.0.bytes()))
    }
}

impl From<[u8; KeyFingerprint::LENGTH]> for KeyFingerprint {
    fn from(raw_bytes: [u8; KeyFingerprint::LENGTH]) -> Self {
        KeyFingerprint(Sha512(raw_bytes))
    }
}

impl From<Sha512> for KeyFingerprint {
    fn from(hash: Sha512) -> Self {
        Self(hash)
    }
}

#[cfg(test)]
impl Distribution<KeyFingerprint> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> KeyFingerprint {
        let mut bytes = [0u8; Sha512::SIZE];
        rng.fill(&mut bytes[..]);
        bytes.into()
    }
}

/// Cryptographic signature.
#[derive(Clone, Deserialize, Eq, Hash, PartialEq, Serialize)]
struct Signature(Vec<u8>);

impl Debug for Signature {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({:10})", HexFmt(&self.0))
    }
}

/// TLS certificate.
///
/// Thin wrapper around `X509` enabling things like Serde serialization and fingerprint caching.
#[derive(Clone, DataSize)]
pub struct TlsCert {
    /// The wrapped x509 certificate.
    #[data_size(skip)] // Skip OpenSSL type.
    x509: X509,

    /// Cached certificate fingerprint.
    cert_fingerprint: CertFingerprint,

    /// Cached public key fingerprint.
    key_fingerprint: KeyFingerprint,
}

impl TlsCert {
    /// Returns the certificate's fingerprint.
    ///
    /// In contrast to the `public_key_fingerprint`, this fingerprint also contains the certificate
    /// information.
    pub(crate) fn fingerprint(&self) -> CertFingerprint {
        self.cert_fingerprint
    }

    /// Returns the public key fingerprint.
    pub(crate) fn public_key_fingerprint(&self) -> KeyFingerprint {
        self.key_fingerprint
    }

    /// Returns a reference to the inner x509 certificate.
    pub(crate) fn as_x509(&self) -> &X509 {
        &self.x509
    }
}

impl Debug for TlsCert {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "TlsCert({:?})", self.fingerprint())
    }
}

impl Hash for TlsCert {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fingerprint().hash(state);
    }
}

impl PartialEq for TlsCert {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint() == other.fingerprint()
    }
}

impl Eq for TlsCert {}

/// Error during loading a x509 certificate.
#[derive(Debug, Error, Serialize)]
pub enum LoadCertError {
    #[error("could not load certificate file: {0}")]
    ReadFile(
        #[serde(skip_serializing)]
        #[source]
        ReadFileError,
    ),
    #[error("unable to load x509 certificate {0:?}")]
    X509CertFromPem(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
}

/// Load a certificate from a file.
pub(crate) fn load_cert<P: AsRef<Path>>(src: P) -> Result<X509, LoadCertError> {
    let pem = read_file(src.as_ref()).map_err(LoadCertError::ReadFile)?;
    X509::from_pem(&pem).map_err(LoadCertError::X509CertFromPem)
}

/// Error during loading a secret key.
#[derive(Debug, Error, Serialize)]
pub(crate) enum LoadSecretKeyError {
    #[error("could not load secret key file: {0}")]
    ReadFile(
        #[serde(skip_serializing)]
        #[source]
        ReadFileError,
    ),
    #[error("unable to load private key from pem {0:?}")]
    PrivateKeyFromPem(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
}

pub(crate) fn load_secret_key<P: AsRef<Path>>(src: P) -> Result<PKey<Private>, LoadSecretKeyError> {
    let pem = read_file(src.as_ref()).map_err(LoadSecretKeyError::ReadFile)?;
    PKey::private_key_from_pem(&pem).map_err(LoadSecretKeyError::PrivateKeyFromPem)
}

/// A signed value.
///
/// Combines a value `V` with a `Signature` and a signature scheme. The signature scheme involves
/// serializing the value to bytes and signing the result.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Signed<V> {
    data: Vec<u8>,
    signature: Signature,
    _phantom: PhantomData<V>,
}

/// Generates a self-signed (key, certificate) pair suitable for TLS and signing.
///
/// The common name of the certificate will be "casper-node".
pub fn generate_node_cert() -> SslResult<(X509, PKey<Private>)> {
    let private_key = generate_private_key()?;
    let cert = generate_cert(&private_key, "casper-node")?;

    Ok((cert, private_key))
}

/// Creates a TLS acceptor for a server.
///
/// The acceptor will restrict TLS parameters to secure one defined in this crate that are
/// compatible with connectors built with `create_tls_connector`.
///
/// Incoming certificates must still be validated using `validate_cert`.
pub(crate) fn create_tls_acceptor(
    cert: &X509Ref,
    private_key: &PKeyRef<Private>,
    keylog: Option<LockedLineWriter>,
) -> SslResult<SslAcceptor> {
    let mut builder = SslAcceptor::mozilla_modern_v5(SslMethod::tls_server())?;
    set_context_options(&mut builder, cert, private_key, keylog)?;

    Ok(builder.build())
}

/// Creates a TLS acceptor for a client.
///
/// A connector compatible with the acceptor created using `create_tls_acceptor`. Server
/// certificates must always be validated using `validate_cert` after connecting.
pub(crate) fn create_tls_connector(
    cert: &X509Ref,
    private_key: &PKeyRef<Private>,
    keylog: Option<LockedLineWriter>,
) -> SslResult<SslConnector> {
    let mut builder = SslConnector::builder(SslMethod::tls_client())?;
    set_context_options(&mut builder, cert, private_key, keylog)?;

    Ok(builder.build())
}

/// Sets common options of both acceptor and connector on TLS context.
///
/// Used internally to set various TLS parameters.
fn set_context_options(
    ctx: &mut SslContextBuilder,
    cert: &X509Ref,
    private_key: &PKeyRef<Private>,
    keylog: Option<LockedLineWriter>,
) -> SslResult<()> {
    ctx.set_min_proto_version(Some(SslVersion::TLS1_3))?;

    ctx.set_certificate(cert)?;
    ctx.set_private_key(private_key)?;
    ctx.check_private_key()?;

    // Note that this does not seem to work as one might naively expect; the client can still send
    // no certificate and there will be no error from OpenSSL. For this reason, we pass set `PEER`
    // (causing the request of a cert), but pass all of them through and verify them after the
    // handshake has completed.
    ctx.set_verify_callback(SslVerifyMode::PEER, |_, _| true);

    if let Some(writer) = keylog {
        ctx.set_keylog_callback(move |_ssl_ref, str| {
            writer.write_line(str);
        });
    }

    Ok(())
}

/// Error during certificate validation.
#[derive(Debug, Error, Serialize)]
pub enum ValidationError {
    /// Failed to read public key from certificate.
    #[error("error reading public key from certificate: {0:?}")]
    CannotReadPublicKey(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Failed to read subject or issuer name.
    #[error("error reading subject or issuer name: {0:?}")]
    CorruptSubjectOrIssuer(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Wrong signature scheme.
    #[error("wrong signature scheme")]
    WrongSignatureAlgorithm,
    /// Failed to read or convert times.
    #[error("there was an issue reading or converting times: {0:?}")]
    TimeIssue(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Certificate not yet valid.
    #[error("the certificate is not yet valid")]
    NotYetValid,
    /// Certificate expired.
    #[error("the certificate expired")]
    Expired,
    /// Serial number could not be compared to the reference.
    #[error("the serial number could not be compared to the reference: {0:?}")]
    InvalidSerialNumber(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Wrong serial number.
    #[error("wrong serial number")]
    WrongSerialNumber,
    /// No valid elliptic curve key could be extracted from certificate.
    #[error("no valid elliptic curve key could be extracted from certificate: {0:?}")]
    CouldNotExtractEcKey(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Public key failed sanity check.
    #[error("the given public key fails basic sanity checks: {0:?}")]
    KeyFailsCheck(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Wrong elliptic curve.
    #[error("underlying elliptic curve is wrong")]
    WrongCurve,
    /// Certificate not self-signed.
    #[error("certificate is not self-signed")]
    NotSelfSigned,
    /// Failed to validate signature.
    #[error("the signature could not be validated")]
    FailedToValidateSignature(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Invalid signature.
    #[error("the signature is invalid")]
    InvalidSignature,
    /// Invalid fingerprint.
    #[error("failed to read fingerprint")]
    InvalidFingerprint(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Failed to create a big num context.
    #[error("could not create a big num context")]
    BigNumContextNotAvailable(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Failed to encode public key.
    #[error("could not encode public key as bytes")]
    PublicKeyEncodingFailed(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Wrong certificate authority.
    #[error("the certificate is not signed by provided certificate authority")]
    WrongCertificateAuthority,
    /// Failed to read public key from certificate.
    #[error("error reading public key from ca certificate: {0:?}")]
    CannotReadCAPublicKey(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
}

/// Checks that the certificate is signed by a provided certificate authority and returns the
/// fingerprint of the public key.
pub(crate) fn validate_cert_with_authority(
    cert: X509,
    ca: &X509,
) -> Result<TlsCert, ValidationError> {
    let authority_key = ca
        .public_key()
        .map_err(ValidationError::CannotReadCAPublicKey)?;

    validate_cert_expiration_date(&cert)?;

    if !cert
        .verify(authority_key.as_ref())
        .map_err(ValidationError::FailedToValidateSignature)?
    {
        return Err(ValidationError::WrongCertificateAuthority);
    }

    // Ensure that the key is using the correct curve parameters.
    tls_cert_from_x509(cert)
}

/// Checks that the cryptographic parameters on a certificate are correct and returns the
/// fingerprint of the public key.
///
/// At the very least this ensures that no weaker ciphers have been used to forge a certificate.
pub(crate) fn validate_self_signed_cert(cert: X509) -> Result<TlsCert, ValidationError> {
    if cert.signature_algorithm().object().nid() != SIGNATURE_ALGORITHM {
        // The signature algorithm is not of the exact kind we are using to generate our
        // certificates, an attacker could have used a weaker one to generate colliding keys.
        return Err(ValidationError::WrongSignatureAlgorithm);
    }
    // TODO: Lock down extensions on the certificate --- if we manage to lock down the whole cert in
    //       a way that no additional bytes can be added (all fields are either known or of fixed
    //       length) we would have an additional hurdle for preimage attacks to clear.

    let subject =
        name_to_string(cert.subject_name()).map_err(ValidationError::CorruptSubjectOrIssuer)?;
    let issuer =
        name_to_string(cert.issuer_name()).map_err(ValidationError::CorruptSubjectOrIssuer)?;
    if subject != issuer {
        // All of our certificates are self-signed, so it cannot hurt to check.
        return Err(ValidationError::NotSelfSigned);
    }

    // All our certificates have serial number 1.
    if !num_eq(cert.serial_number(), 1).map_err(ValidationError::InvalidSerialNumber)? {
        return Err(ValidationError::WrongSerialNumber);
    }

    // Check expiration times against current time.
    validate_cert_expiration_date(&cert)?;

    // Ensure that the key is using the correct curve parameters.
    let (public_key, ec_key) = validate_cert_ec_key(&cert)?;
    if ec_key.group().curve_name() != Some(SIGNATURE_CURVE) {
        // The underlying curve is not the one we chose.
        return Err(ValidationError::WrongCurve);
    }

    // Finally we can check the actual signature.
    if !cert
        .verify(&public_key)
        .map_err(ValidationError::FailedToValidateSignature)?
    {
        return Err(ValidationError::InvalidSignature);
    }

    tls_cert_from_x509_and_key(cert, ec_key)
}

/// Creates a [`TlsCert`] instance from [`X509`] cert instance.
///
/// This function only ensures that the cert contains EC public key, and is suitable for quickly
/// validating certs signed by CA.
pub(crate) fn tls_cert_from_x509(cert: X509) -> Result<TlsCert, ValidationError> {
    let (_public_key, ec_key) = validate_cert_ec_key(&cert)?;
    tls_cert_from_x509_and_key(cert, ec_key)
}

fn tls_cert_from_x509_and_key(
    cert: X509,
    ec_key: EcKey<Public>,
) -> Result<TlsCert, ValidationError> {
    let cert_fingerprint = cert_fingerprint(&cert)?;
    let key_fingerprint = key_fingerprint(ec_key)?;
    Ok(TlsCert {
        x509: cert,
        cert_fingerprint,
        key_fingerprint,
    })
}

/// Calculate a fingerprint for the X509 certificate.
pub(crate) fn cert_fingerprint(cert: &X509) -> Result<CertFingerprint, ValidationError> {
    assert_eq!(Sha512::NID, SIGNATURE_DIGEST);
    let digest = &cert
        .digest(Sha512::create_message_digest())
        .map_err(ValidationError::InvalidFingerprint)?;
    let cert_fingerprint = CertFingerprint(Sha512::from_openssl_digest(digest));
    Ok(cert_fingerprint)
}

/// Calculate a fingerprint for the public EC key.
pub(crate) fn key_fingerprint(ec_key: EcKey<Public>) -> Result<KeyFingerprint, ValidationError> {
    let mut big_num_context =
        BigNumContext::new().map_err(ValidationError::BigNumContextNotAvailable)?;
    let buf = ec_key
        .public_key()
        .to_bytes(
            ec::EcGroup::from_curve_name(SIGNATURE_CURVE)
                .expect("broken constant SIGNATURE_CURVE")
                .as_ref(),
            ec::PointConversionForm::COMPRESSED,
            &mut big_num_context,
        )
        .map_err(ValidationError::PublicKeyEncodingFailed)?;
    let key_fingerprint = KeyFingerprint(Sha512::new(&buf));
    Ok(key_fingerprint)
}

/// Validate cert's public key, and it's EC key parameters.
fn validate_cert_ec_key(cert: &X509) -> Result<(PKey<Public>, EcKey<Public>), ValidationError> {
    let public_key = cert
        .public_key()
        .map_err(ValidationError::CannotReadPublicKey)?;
    let ec_key = public_key
        .ec_key()
        .map_err(ValidationError::CouldNotExtractEcKey)?;
    ec_key.check_key().map_err(ValidationError::KeyFailsCheck)?;
    Ok((public_key, ec_key))
}

/// Check cert's expiration times against current time.
fn validate_cert_expiration_date(cert: &X509) -> Result<(), ValidationError> {
    let asn1_now = Asn1Time::from_unix(now()).map_err(ValidationError::TimeIssue)?;
    if asn1_now
        .compare(cert.not_before())
        .map_err(ValidationError::TimeIssue)?
        != Ordering::Greater
    {
        return Err(ValidationError::NotYetValid);
    }

    if asn1_now
        .compare(cert.not_after())
        .map_err(ValidationError::TimeIssue)?
        != Ordering::Less
    {
        return Err(ValidationError::Expired);
    }

    Ok(())
}

/// Returns an OpenSSL compatible timestamp.
fn now() -> i64 {
    // Note: We could do the timing dance a little better going straight to the UNIX time functions,
    //       but this saves us having to bring in `libc` as a dependency.
    let now = SystemTime::now();
    let ts: i64 = now
        .duration_since(UNIX_EPOCH)
        // This should work unless the clock is set to before 1970.
        .expect("Great Scott! Your clock is horribly broken, Marty.")
        .as_secs()
        // This will fail past year 2038 on 32 bit systems and very far into the future, both cases
        // we consider out of scope.
        .try_into()
        .expect("32-bit systems and far future are not supported");

    ts
}

/// Creates an ASN1 integer from a `u32`.
fn mknum(n: u32) -> Result<Asn1Integer, ErrorStack> {
    let bn = BigNum::from_u32(n)?;

    bn.to_asn1_integer()
}

/// Creates an ASN1 name from string components.
///
/// If `c` or `o` are empty string, they are omitted from the result.
fn mkname(c: &str, o: &str, cn: &str) -> Result<X509Name, ErrorStack> {
    let mut builder = X509NameBuilder::new()?;

    if !c.is_empty() {
        builder.append_entry_by_text("C", c)?;
    }

    if !o.is_empty() {
        builder.append_entry_by_text("O", o)?;
    }

    builder.append_entry_by_text("CN", cn)?;
    Ok(builder.build())
}

/// Converts an `X509NameRef` to a human readable string.
fn name_to_string(name: &X509NameRef) -> SslResult<String> {
    let mut output = String::new();

    for entry in name.entries() {
        output.push_str(entry.object().nid().long_name()?);
        output.push('=');
        output.push_str(entry.data().as_utf8()?.as_ref());
        output.push(' ');
    }

    Ok(output)
}

/// Checks if an `Asn1IntegerRef` is equal to a given u32.
fn num_eq(num: &Asn1IntegerRef, other: u32) -> SslResult<bool> {
    let l = num.to_bn()?;
    let r = BigNum::from_u32(other)?;

    // The `BigNum` API seems to be really lacking here.
    Ok(l.is_negative() == r.is_negative() && l.ucmp(r.as_ref()) == Ordering::Equal)
}

/// Generates a secret key suitable for TLS encryption.
fn generate_private_key() -> SslResult<PKey<Private>> {
    // We do not care about browser-compliance, so we're free to use elliptic curves that are more
    // likely to hold up under pressure than the NIST ones. We want to go with ED25519 because djb
    // knows best: PKey::generate_ed25519()
    //
    // However the following bug currently prevents us from doing so:
    // https://mta.openssl.org/pipermail/openssl-users/2018-July/008362.html (The same error occurs
    // when trying to sign the cert inside the builder)

    // Our second choice is 2^521-1, which is slow but a "nice prime".
    // http://blog.cr.yp.to/20140323-ecdsa.html

    // An alternative is https://en.bitcoin.it/wiki/Secp256k1, which puts us at level of bitcoin.

    // TODO: Please verify this for accuracy!

    let ec_group = ec::EcGroup::from_curve_name(SIGNATURE_CURVE)?;
    let ec_key = ec::EcKey::generate(ec_group.as_ref())?;

    PKey::from_ec_key(ec_key)
}

/// Generates a self-signed certificate based on `private_key` with given CN.
fn generate_cert(private_key: &PKey<Private>, cn: &str) -> SslResult<X509> {
    let mut builder = X509Builder::new()?;

    // x509 v3 commonly used, the version is 0-indexed, thus 2 == v3.
    builder.set_version(2)?;

    // The serial number is always one, since we are issuing only one cert.
    builder.set_serial_number(mknum(1)?.as_ref())?;

    let issuer = mkname("US", "Casper Blockchain", cn)?;

    // Set the issuer, subject names, putting the "self" in "self-signed".
    builder.set_issuer_name(issuer.as_ref())?;
    builder.set_subject_name(issuer.as_ref())?;

    let ts = now();
    // We set valid-from to one minute into the past to allow some clock-skew.
    builder.set_not_before(Asn1Time::from_unix(ts - 60)?.as_ref())?;

    // Valid-until is a little under 10 years, missing at least 2 leap days.
    builder.set_not_after(Asn1Time::from_unix(ts + 10 * 365 * 24 * 60 * 60)?.as_ref())?;

    // Set the public key and sign.
    builder.set_pubkey(private_key.as_ref())?;
    assert_eq!(Sha512::NID, SIGNATURE_DIGEST);
    builder.sign(private_key.as_ref(), Sha512::create_message_digest())?;

    let cert = builder.build();

    // Cheap sanity check.
    assert!(
        validate_self_signed_cert(cert.clone()).is_ok(),
        "newly generated cert does not pass our own validity check"
    );

    Ok(cert)
}

// Below are trait implementations for signatures and fingerprints. Both implement the full set of
// traits that are required to stick into either a `HashMap` or `BTreeMap`.
impl PartialEq for Sha512 {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
    }
}

impl Eq for Sha512 {}

impl Ord for Sha512 {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(self.bytes(), other.bytes())
    }
}

impl PartialOrd for Sha512 {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(Ord::cmp(self, other))
    }
}

impl Debug for Sha512 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0[..]))
    }
}

impl Display for Sha512 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(&self.0[..]))
    }
}

impl Display for CertFingerprint {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Display for KeyFingerprint {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(self.0.bytes()))
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(&self.0[..]))
    }
}

impl<T> Display for Signed<T>
where
    T: Display + for<'de> Deserialize<'de>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // Decode the data here, even if it is expensive.
        match bincode::deserialize::<T>(self.data.as_slice()) {
            Ok(item) => write!(f, "signed[{}]<{} bytes>", self.signature, item),
            Err(_err) => write!(f, "signed[{}]<CORRUPT>", self.signature),
        }
    }
}

// Since all `Sha512`s are already hashes, we provide a very cheap hashing function that uses
// bytes from the fingerprint as input, cutting the number of bytes to be hashed to 1/16th.

// If this is ever a performance bottleneck, a custom hasher can be added that passes these bytes
// through unchanged.
impl Hash for Sha512 {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Use the first eight bytes when hashing, giving 64 bits pure entropy.
        let mut chunk = [0u8; 8];

        // TODO: Benchmark if this is really worthwhile over the automatic derivation.
        chunk.copy_from_slice(&self.bytes()[0..8]);

        state.write_u64(u64::from_le_bytes(chunk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_name_to_string() {
        let name = mkname("sc", "some_org", "some_cn").expect("could not create name");

        assert_eq!(
            name_to_string(name.as_ref()).expect("name to string failed"),
            "countryName=sc organizationName=some_org commonName=some_cn "
        );
    }

    #[test]
    fn test_validate_self_signed_cert() {
        let (cert, private_key) = generate_node_cert().expect("failed to generate key, cert pair");

        // Validates self signed cert
        let _tls_cert =
            validate_self_signed_cert(cert).expect("generated self signed cert is not valid");

        // Cert signed by a CA does not validate as self signed
        let ca_private_key = generate_private_key().expect("failed to generate private key");
        let ca_signed_cert = make_ca_signed_cert(private_key, ca_private_key);

        let error = validate_self_signed_cert(ca_signed_cert)
            .expect_err("should not validate ca signed cert as self signed");
        assert!(
            matches!(error, ValidationError::InvalidSignature),
            "{:?}",
            error
        );
    }

    #[test]
    fn test_validate_cert_with_authority() {
        let (ca_cert, ca_private_key) =
            generate_node_cert().expect("failed to generate key, cert pair");

        let (different_ca_cert, _ca_private_key) =
            generate_node_cert().expect("failed to generate key, cert pair");

        let node_private_key = generate_private_key().expect("failed to generate private key");

        let node_cert = make_ca_signed_cert(node_private_key, ca_private_key);

        validate_self_signed_cert(node_cert.clone())
            .expect_err("should not validate CA signed cert as self signed");

        let _node_tls_cert = validate_cert_with_authority(node_cert.clone(), &ca_cert)
            .expect("should validate with ca cert");

        let validation_error = validate_cert_with_authority(node_cert, &different_ca_cert)
            .expect_err("should not validate cert against different CA");

        assert!(
            matches!(validation_error, ValidationError::WrongCertificateAuthority),
            "{:?}",
            validation_error
        );
    }

    fn make_ca_signed_cert(private_key: PKey<Private>, ca_private_key: PKey<Private>) -> X509 {
        let mut builder = X509Builder::new().unwrap();
        builder.set_version(2).unwrap();
        builder
            .set_serial_number(mknum(1).unwrap().as_ref())
            .unwrap();
        let issuer = mkname("US", "Casper Blockchain", "Casper Network").unwrap();
        builder.set_issuer_name(issuer.as_ref()).unwrap();
        builder.set_subject_name(issuer.as_ref()).unwrap();
        let ts = now();
        builder
            .set_not_before(Asn1Time::from_unix(ts - 60).unwrap().as_ref())
            .unwrap();
        builder
            .set_not_after(
                Asn1Time::from_unix(ts + 10 * 365 * 24 * 60 * 60)
                    .unwrap()
                    .as_ref(),
            )
            .unwrap();
        builder.set_pubkey(private_key.as_ref()).unwrap();
        assert_eq!(Sha512::NID, SIGNATURE_DIGEST);
        builder
            .sign(ca_private_key.as_ref(), Sha512::create_message_digest())
            .unwrap();
        builder.build()
    }
}
