//! Transport layer security and signing based on OpenSSL.
//!
//! This module wraps some of the lower-level TLS constructs to provide a reasonably safe to use API
//! surface for the rest of the application. It also fixates the security parameters of the TLS
//! level in a central place.
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

use anyhow::Context;
use displaydoc::Display;
use hex_fmt::HexFmt;
use openssl::{asn1, bn, ec, error, hash, nid, pkey, sha, sign, ssl, x509};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_big_array::big_array;
use std::convert::TryInto;
use std::hash::Hash;
use std::marker::PhantomData;
use std::{cmp, fmt, path, str, time};
use thiserror::Error;

big_array! { BigArray; }

/// The chosen signature algorithm (**ECDSA  with SHA512**).
const SIGNATURE_ALGORITHM: nid::Nid = nid::Nid::ECDSA_WITH_SHA512;

/// The underlying elliptic curve (**P-521**).
const SIGNATURE_CURVE: nid::Nid = nid::Nid::SECP521R1;

/// The chosen signature algorithm (**SHA512**).
const SIGNATURE_DIGEST: nid::Nid = nid::Nid::SHA512;

/// OpenSSL result type alias.
///
/// Many functions rely solely on `openssl` functions and return this kind of result.
pub type SslResult<T> = Result<T, error::ErrorStack>;

/// SHA512 hash.
#[derive(Copy, Clone, Deserialize, Serialize)]
pub struct Sha512(#[serde(with = "BigArray")] [u8; Sha512::SIZE]);

/// Public key fingerprint
#[derive(Copy, Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Fingerprint(Sha512);

/// Cryptographic signature.
#[derive(Copy, Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Signature(Sha512);

/// TLS certificate.
///
/// Thin wrapper around `X509` enabling things like serde Serialization.
#[derive(Deserialize, Serialize)]
pub struct TlsCert(#[serde(with = "x509_serde")] pub openssl::x509::X509);

/// A signed value.
///
/// Combines a value `V` with a `Signature` and a signature scheme. The signature scheme involves
/// serializing the value to bytes and signing the result.
#[derive(Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Signed<V> {
    data: Vec<u8>,
    signature: Signature,
    _phantom: PhantomData<V>,
}

impl<V> Signed<V>
where
    V: Serialize,
{
    /// Create new signed value.
    ///
    /// Serializes the value to a buffer and signs the buffer.
    pub fn new(value: &V, signing_key: &pkey::PKeyRef<pkey::Private>) -> anyhow::Result<Self> {
        let data = rmp_serde::to_vec(value)?;
        let signature = Signature::create(signing_key, &data)?;

        Ok(Signed {
            data,
            signature,
            _phantom: PhantomData,
        })
    }
}

impl Sha512 {
    /// Size of digest in bytes.
    pub const SIZE: usize = 64;

    /// OpenSSL NID.
    const NID: nid::Nid = nid::Nid::SHA512;

    /// Create a new Sha512 by hashing a slice.
    pub fn new<B: AsRef<[u8]>>(data: B) -> Self {
        let mut openssl_sha = sha::Sha512::new();
        openssl_sha.update(data.as_ref());
        Sha512(openssl_sha.finish())
    }

    /// Return bytestring of the hash, with length `Self::SIZE`.
    pub fn bytes(&self) -> &[u8] {
        let bs = &self.0[..];

        debug_assert_eq!(bs.len(), Self::SIZE);
        bs
    }

    /// Convert an OpenSSL digest into an `Sha512`.
    fn from_openssl_digest(digest: &hash::DigestBytes) -> Self {
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

    /// Return a new OpenSSL `MessageDigest` set to SHA-512.
    fn create_message_digest() -> hash::MessageDigest {
        // This can only fail if we specify a `Nid` that does not exist, which cannot happen unless
        // there is something wrong with `Self::NID`.
        hash::MessageDigest::from_nid(Self::NID).expect("Sha512::NID is invalid")
    }
}

impl<V> Signed<V>
where
    V: DeserializeOwned,
{
    /// Validate signature and restore value.
    pub fn validate(&self, public_key: &pkey::PKeyRef<pkey::Public>) -> anyhow::Result<V> {
        if self.signature.verify(public_key, &self.data)? {
            Ok(rmp_serde::from_read(self.data.as_slice())?)
        } else {
            Err(anyhow::anyhow!("invalid signature"))
        }
    }
}

impl Signature {
    /// Sign a binary blob with the blessed ciphers and TLS parameters.
    pub fn create(private_key: &pkey::PKeyRef<pkey::Private>, data: &[u8]) -> SslResult<Self> {
        // TODO: This needs verification to ensure we're not doing stupid/textbook RSA-ish.

        // Sha512 is hardcoded, so check we're creating the correct signature.
        assert_eq!(Sha512::NID, SIGNATURE_DIGEST);

        let mut signer = sign::Signer::new(Sha512::create_message_digest(), private_key)?;

        let mut sig_buf = [0; Sha512::SIZE];

        let written = signer.sign_oneshot(&mut sig_buf[..], data)?;
        assert_eq!(written, Sha512::SIZE);

        Ok(Signature(Sha512(sig_buf)))
    }

    /// Verify that signature matches on a binary blob.
    pub fn verify(
        self: &Signature,
        public_key: &pkey::PKeyRef<pkey::Public>,
        data: &[u8],
    ) -> SslResult<bool> {
        assert_eq!(Sha512::NID, SIGNATURE_DIGEST);

        let mut verifier = sign::Verifier::new(Sha512::create_message_digest(), public_key)?;

        verifier.verify_oneshot(self.0.bytes(), data)
    }
}

impl TlsCert {
    /// Wrap X509 certificate.
    fn new(x509: x509::X509) -> Result<Self, ValidationError> {
        validate_cert(&x509)?;
        // Ensure the certificate can extract a valid public key.
        Ok(TlsCert(x509))
    }

    /// Validate certificate, returning the fingerprint.
    fn validate(&self) -> Result<Fingerprint, ValidationError> {
        validate_cert(self.x509())
    }

    /// Return the certificates fingerprint.
    ///
    /// In contrast to the `public_key_fingerprint`, this fingerprint also contains the certificate
    /// information.
    fn fingerprint(&self) -> Fingerprint {
        let digest = &self
            .0
            .digest(hash::MessageDigest::from_nid(Sha512::NID).expect("SHA512 NID not found"))
            .expect("TlsCert does not have fingerprint digest, this should not happen");
        Fingerprint(Sha512::from_openssl_digest(digest))
    }

    /// Extract the public key from the certificate.
    fn public_key(&self) -> pkey::PKey<pkey::Public> {
        // This can never fail, we validate the certificate on construction and deserialization.
        self.0
            .public_key()
            .expect("public key extraction failed, how did we end up with an invalid cert?")
    }

    /// Generate a fingerprint by hashing the public key.
    fn public_key_fingerprint(&self) -> SslResult<Fingerprint> {
        let mut big_num_context = bn::BigNumContext::new()?;

        let buf = self.public_key().ec_key()?.public_key().to_bytes(
            ec::EcGroup::from_curve_name(SIGNATURE_CURVE)?.as_ref(),
            ec::PointConversionForm::COMPRESSED,
            &mut big_num_context,
        )?;

        Ok(Fingerprint(Sha512::new(&buf)))
    }

    /// Return OpenSSL X509 certificate.
    fn x509(&self) -> &x509::X509 {
        &self.0
    }
}

impl fmt::Debug for TlsCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

/// Generate a self-signed (key, certificate) pair suitable for TLS and signing.
///
/// The common name of the certificate will be "casper-node".
pub fn generate_node_cert() -> SslResult<(x509::X509, pkey::PKey<pkey::Private>)> {
    let private_key = generate_private_key()?;
    let cert = generate_cert(&private_key, "casper-node")?;

    Ok((cert, private_key))
}

/// Create a TLS acceptor for a server.
///
/// The acceptor will restrict TLS parameters to secure one defined in this crate that are
/// compatible with connectors built with `create_tls_connector`.
///
/// Incoming certificates must still be validated using `validate_cert`.
pub fn create_tls_acceptor(
    cert: &x509::X509Ref,
    private_key: &pkey::PKeyRef<pkey::Private>,
) -> SslResult<ssl::SslAcceptor> {
    let mut builder = ssl::SslAcceptor::mozilla_modern_v5(ssl::SslMethod::tls_server())?;
    set_context_options(&mut builder, cert, private_key)?;

    Ok(builder.build())
}

/// Create a TLS acceptor for a client.
///
/// A connector compatible with the acceptor created using `create_tls_acceptor`. Server
/// certificates must always be validated using `validate_cert` after connecting.
pub fn create_tls_connector(
    cert: &x509::X509Ref,
    private_key: &pkey::PKeyRef<pkey::Private>,
) -> SslResult<ssl::SslConnector> {
    let mut builder = ssl::SslConnector::builder(ssl::SslMethod::tls_client())?;
    set_context_options(&mut builder, cert, private_key)?;

    Ok(builder.build())
}

/// Set common options of both acceptor and connector on TLS context.
///
/// Used internally to set various TLS parameters.
fn set_context_options(
    ctx: &mut ssl::SslContextBuilder,
    cert: &x509::X509Ref,
    private_key: &pkey::PKeyRef<pkey::Private>,
) -> SslResult<()> {
    ctx.set_min_proto_version(Some(ssl::SslVersion::TLS1_3))?;

    ctx.set_certificate(cert)?;
    ctx.set_private_key(private_key)?;
    ctx.check_private_key()?;

    // Note that this does not seem to work as one might naively expect; the client can still send
    // no certificate and there will be no error from OpenSSL. For this reason, we pass set `PEER`
    // (causing the request of a cert), but pass all of them through and verify them after the
    // handshake has completed.
    ctx.set_verify_callback(ssl::SslVerifyMode::PEER, |_, _| true);

    Ok(())
}

/// Error during certificate validation.
#[derive(Debug, Display, Error)]
pub enum ValidationError {
    /// error reading public key from certificate: {0:?}
    CannotReadPublicKey(#[source] error::ErrorStack),
    /// error reading subject or issuer name: {0:?}
    CorruptSubjectOrIssuer(#[source] error::ErrorStack),
    /// wrong signature scheme
    WrongSignatureAlgorithm,
    /// there was an issue reading or converting times: {0:?}
    TimeIssue(#[source] error::ErrorStack),
    /// the certificate is not yet valid
    NotYetValid,
    /// the certificate expired
    Expired,
    /// the serial number could not be compared to the reference: {0:?}
    InvalidSerialNumber(#[source] error::ErrorStack),
    /// wrong serial number
    WrongSerialNumber,
    /// no valid elliptic curve key could be extracted from cert: {0:?}
    CouldNotExtractEcKey(#[source] error::ErrorStack),
    /// the given public key fails basic sanity checks: {0:?}
    KeyFailsCheck(#[source] error::ErrorStack),
    /// underlying elliptic curve is wrong
    WrongCurve,
    /// certificate is not self-signed
    NotSelfSigned,
    /// the signature could not be validated
    FailedToValidateSignature(#[source] error::ErrorStack),
    /// the signature is invalid
    InvalidSignature,
    /// failed to read fingerprint
    InvalidFingerprint(#[source] error::ErrorStack),
}

/// Check that the cryptographic parameters on a certificate are correct and return the fingerprint.
///
/// At the very least this ensures that no weaker ciphers have been used to forge a certificate.
pub fn validate_cert(cert: &x509::X509Ref) -> Result<Fingerprint, ValidationError> {
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
    let asn1_now = asn1::Asn1Time::from_unix(now()).map_err(ValidationError::TimeIssue)?;
    if asn1_now
        .compare(cert.not_before())
        .map_err(ValidationError::TimeIssue)?
        != cmp::Ordering::Greater
    {
        return Err(ValidationError::NotYetValid);
    }

    if asn1_now
        .compare(cert.not_after())
        .map_err(ValidationError::TimeIssue)?
        != cmp::Ordering::Less
    {
        return Err(ValidationError::Expired);
    }

    // Ensure that the key is using the correct curve parameters.
    let public_key = cert
        .public_key()
        .map_err(ValidationError::CannotReadPublicKey)?;

    let ec_key = public_key
        .ec_key()
        .map_err(ValidationError::CouldNotExtractEcKey)?;

    ec_key.check_key().map_err(ValidationError::KeyFailsCheck)?;
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

    // We now have a valid certificate and can extract the fingerprint.
    assert_eq!(Sha512::NID, SIGNATURE_DIGEST);
    let digest = &cert
        .digest(Sha512::create_message_digest())
        .map_err(ValidationError::InvalidFingerprint)?;
    Ok(Fingerprint(Sha512::from_openssl_digest(digest)))
}

/// Load a certificate from a file.
pub fn load_cert<P: AsRef<path::Path>>(src: P) -> anyhow::Result<x509::X509> {
    let pem = std::fs::read(src.as_ref())
        .with_context(|| format!("failed to load certificate {:?}", src.as_ref()))?;

    Ok(x509::X509::from_pem(&pem).context("parsing certificate")?)
}

/// Load a private key from a file.
pub fn load_private_key<P: AsRef<path::Path>>(src: P) -> anyhow::Result<pkey::PKey<pkey::Private>> {
    let pem = std::fs::read(src.as_ref())
        .with_context(|| format!("failed to load private key {:?}", src.as_ref()))?;

    // TODO: It might be that we need to call `pkey::PKey::private_key_from_pkcs8` instead.
    Ok(pkey::PKey::private_key_from_pem(&pem).context("parsing private key")?)
}

/// Save a certificate to a file.
pub fn save_cert<P: AsRef<path::Path>>(cert: &x509::X509Ref, dest: P) -> anyhow::Result<()> {
    let pem = cert.to_pem().context("converting certificate to PEM")?;

    std::fs::write(dest.as_ref(), pem)
        .with_context(|| format!("failed to write certificate {:?}", dest.as_ref()))?;
    Ok(())
}

/// Save a private key to a file.
pub fn save_private_key<P: AsRef<path::Path>>(
    key: &pkey::PKeyRef<pkey::Private>,
    dest: P,
) -> anyhow::Result<()> {
    let pem = key
        .private_key_to_pem_pkcs8()
        .context("converting private key to PEM")?;

    std::fs::write(dest.as_ref(), pem)
        .with_context(|| format!("failed to write private key {:?}", dest.as_ref()))?;
    Ok(())
}

/// Return an OpenSSL compatible timestamp.
///

fn now() -> i64 {
    // Note: We could do the timing dance a little better going straight to the UNIX time functions,
    //       but this saves us having to bring in `libc` as a dependency.
    let now = time::SystemTime::now();
    let ts: i64 = now
        .duration_since(time::UNIX_EPOCH)
        // This should work unless the clock is set to before 1970.
        .expect("Great Scott! Your clock is horribly broken, Marty.")
        .as_secs()
        // This will fail past year 2038 on 32 bit systems and a very far into the future, both
        // cases we consider out of scope.
        .try_into()
        .expect("32-bit systems and far future are not supported");

    ts
}

/// Create an ASN1 integer from a `u32`.
fn mknum(n: u32) -> Result<asn1::Asn1Integer, error::ErrorStack> {
    let bn = openssl::bn::BigNum::from_u32(n)?;

    bn.to_asn1_integer()
}

/// Create an ASN1 name from string components.
///
/// If `c` or `o` are empty string, they are omitted from the result.
fn mkname(c: &str, o: &str, cn: &str) -> Result<x509::X509Name, error::ErrorStack> {
    let mut builder = x509::X509NameBuilder::new()?;

    if !c.is_empty() {
        builder.append_entry_by_text("C", c)?;
    }

    if !o.is_empty() {
        builder.append_entry_by_text("O", o)?;
    }

    builder.append_entry_by_text("CN", cn)?;
    Ok(builder.build())
}

/// Convert an `X509NameRef` to a human readable string.
fn name_to_string(name: &x509::X509NameRef) -> SslResult<String> {
    let mut output = String::new();

    for entry in name.entries() {
        output.push_str(entry.object().nid().long_name()?);
        output.push_str("=");
        output.push_str(entry.data().as_utf8()?.as_ref());
        output.push_str(" ");
    }

    Ok(output)
}

/// Check if an `Asn1IntegerRef` is equal to a given u32.
fn num_eq(num: &asn1::Asn1IntegerRef, other: u32) -> SslResult<bool> {
    let l = num.to_bn()?;
    let r = bn::BigNum::from_u32(other)?;

    // The `BigNum` API seems to be really lacking here.
    Ok(l.is_negative() == r.is_negative() && l.ucmp(&r.as_ref()) == cmp::Ordering::Equal)
}

/// Generate a secret key suitable for TLS encryption.
fn generate_private_key() -> SslResult<pkey::PKey<pkey::Private>> {
    // We do not care about browser-compliance, so we're free to use elliptic curves that are more
    // likely to hold up under pressure than the NIST ones. We want to go with ED25519 because djb
    // know's best: pkey::PKey::generate_ed25519()
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

    pkey::PKey::from_ec_key(ec_key)
}

/// Generate a self-signed certificate based on `private_key` with given CN.
fn generate_cert(private_key: &pkey::PKey<pkey::Private>, cn: &str) -> SslResult<x509::X509> {
    let mut builder = x509::X509Builder::new()?;

    // x509 v3 commonly used, the version is 0-indexed, thus 2 == v3.
    builder.set_version(2)?;

    // The serial number is always one, since we are issuing only one cert.
    builder.set_serial_number(mknum(1)?.as_ref())?;

    let issuer = mkname("US", "CasperLabs Blockchain", cn)?;

    // Set the issuer, subject names, putting the "self" in "self-signed".
    builder.set_issuer_name(issuer.as_ref())?;
    builder.set_subject_name(issuer.as_ref())?;

    let ts = now();
    // We set valid-from to one minute into the past to allow some clock-skew.
    builder.set_not_before(asn1::Asn1Time::from_unix(ts - 60)?.as_ref())?;

    // Valid-until is a little under 10 years, missing at least 2 leap days.
    builder.set_not_after(asn1::Asn1Time::from_unix(ts + 10 * 365 * 24 * 60 * 60)?.as_ref())?;

    // Set the public key and sign.
    builder.set_pubkey(private_key.as_ref())?;
    assert_eq!(Sha512::NID, SIGNATURE_DIGEST);
    builder.sign(private_key.as_ref(), Sha512::create_message_digest())?;

    let cert = builder.build();

    // Cheap sanity check.
    assert!(
        validate_cert(&cert).is_ok(),
        "newly generated cert does not pass our own validity check"
    );

    Ok(cert)
}

/// Serde support for `openssl::x509::X509` certificates.
///
/// Will also check if certificates are valid according to `validate_cert` when deserializing.
mod x509_serde {
    use super::validate_cert;
    use openssl::x509::X509;
    use serde;
    use std::str;

    /// Serde-compatible serialization for X509 certificates.
    pub fn serialize<S>(value: &X509, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let encoded = value.to_pem().map_err(serde::ser::Error::custom)?;

        // We don't expect encoding to fail, since PEMs are ASCII, but pass the error just in case.
        serializer.serialize_str(str::from_utf8(&encoded).map_err(serde::ser::Error::custom)?)
    }

    /// Serde-compatible deserialization for X509 certificates.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<X509, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;

        // Create an extra copy for simplicity here. If this becomes a bottleneck, feel free to try
        // to leverage Cow<str> here, or implement a custom visitor that handles both cases.
        let s: String = Deserialize::deserialize(deserializer)?;
        let x509 = X509::from_pem(s.as_bytes()).map_err(serde::de::Error::custom)?;

        validate_cert(&x509).map_err(serde::de::Error::custom)?;
        Ok(x509)
    }
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
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        Ord::cmp(self.bytes(), other.bytes())
    }
}

impl PartialOrd for Sha512 {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(Ord::cmp(self, other))
    }
}

impl fmt::Debug for Sha512 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", HexFmt(&self.0[..]))
    }
}

impl fmt::Display for Sha512 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", HexFmt(&self.0[0..7]))
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
mod test {
    use super::{generate_node_cert, mkname, name_to_string, TlsCert};

    #[test]
    fn simple_name_to_string() {
        let name = mkname("sc", "some_org", "some_cn").expect("could not create name");

        assert_eq!(
            name_to_string(name.as_ref()).expect("name to string failed"),
            "countryName=sc organizationName=some_org commonName=some_cn "
        );
    }

    #[test]
    fn test_tls_cert_serde_roundtrip() {
        let (cert, _private_key) = generate_node_cert().expect("failed to generate key, cert pair");

        let tls_cert = TlsCert(cert);

        // There is no `PartialEq` impl for `TlsCert`, so we simply serialize it twice.
        let serialized = rmp_serde::to_vec(&tls_cert).expect("could not serialize");
        let deserialized: TlsCert =
            rmp_serde::from_read(serialized.as_slice()).expect("could not deserialize");
        let serialized_again = rmp_serde::to_vec(&deserialized).expect("could not serialize");

        assert_eq!(serialized, serialized_again);
    }
}
