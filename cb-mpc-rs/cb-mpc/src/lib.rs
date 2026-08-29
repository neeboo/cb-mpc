//! Safe Rust API for Coinbase cb-mpc.
//!
//! The cryptographic implementation is the upstream C++ library. This crate
//! owns serialized key-share material and delegates all unsafe work to
//! `cb-mpc-sys`.

use std::collections::HashSet;
use std::fmt;
use zeroize::Zeroize;

pub use cb_mpc_sys::{Transport, TransportError};

#[derive(Debug)]
pub enum Error {
    InvalidPolicy,
    InvalidParty,
    InvalidDigestLength,
    Native(cb_mpc_sys::Error),
}

impl Error {
    pub fn native_code(&self) -> Option<i32> {
        match self {
            Self::Native(cb_mpc_sys::Error::C(code)) => Some(*code),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("invalid threshold policy"),
            Self::InvalidParty => formatter.write_str("party or quorum does not match the policy"),
            Self::InvalidDigestLength => formatter.write_str("ECDSA requires a 32-byte prehash"),
            Self::Native(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Native(error) => Some(error),
            _ => None,
        }
    }
}

impl From<cb_mpc_sys::Error> for Error {
    fn from(error: cb_mpc_sys::Error) -> Self {
        Self::Native(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdPolicy {
    threshold: usize,
    party_names: Vec<String>,
}

impl ThresholdPolicy {
    pub fn new(threshold: usize, party_names: Vec<String>) -> Result<Self, Error> {
        if threshold == 0 || threshold > party_names.len() || party_names.is_empty() {
            return Err(Error::InvalidPolicy);
        }
        let mut unique = HashSet::with_capacity(party_names.len());
        if party_names.iter().any(|name| {
            name.is_empty() || name.as_bytes().contains(&0) || !unique.insert(name.clone())
        }) {
            return Err(Error::InvalidPolicy);
        }
        Ok(Self {
            threshold,
            party_names,
        })
    }

    pub fn threshold(&self) -> usize {
        self.threshold
    }

    pub fn party_names(&self) -> &[String] {
        &self.party_names
    }

    fn validate_quorum(&self, quorum: &[&str]) -> Result<Vec<String>, Error> {
        if quorum.len() < self.threshold {
            return Err(Error::InvalidParty);
        }
        let mut unique = HashSet::with_capacity(quorum.len());
        if quorum.iter().any(|name| {
            !self.party_names.iter().any(|party| party == name) || !unique.insert(*name)
        }) {
            return Err(Error::InvalidParty);
        }
        Ok(quorum.iter().map(|name| (*name).to_owned()).collect())
    }

    fn validate_online_parties(&self, online: &[String]) -> Result<(), Error> {
        if online.len() < self.threshold {
            return Err(Error::InvalidParty);
        }
        let mut unique = HashSet::with_capacity(online.len());
        if online
            .iter()
            .any(|name| !self.party_names.contains(name) || !unique.insert(name.as_str()))
        {
            return Err(Error::InvalidParty);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionId(Vec<u8>);

impl SessionId {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Owned secret bytes which are zeroized before deallocation.
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Explicitly expose the secret to code that persists or passes it onward.
    pub fn expose_secret(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SecretBytes(REDACTED, len={})", self.0.len())
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.as_mut_slice().zeroize();
        #[cfg(test)]
        SECRET_DROP_WAS_ZEROIZED.set(self.0.iter().all(|byte| *byte == 0));
    }
}

#[cfg(test)]
thread_local! {
    static SECRET_DROP_WAS_ZEROIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn reset_secret_drop_probe() {
    SECRET_DROP_WAS_ZEROIZED.set(false);
}

#[cfg(test)]
fn last_secret_drop_was_zeroized() -> bool {
    SECRET_DROP_WAS_ZEROIZED.get()
}

pub struct EcdsaKeyShare(SecretBytes);

impl fmt::Debug for EcdsaKeyShare {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("EcdsaKeyShare")
            .field(&"REDACTED")
            .finish()
    }
}

impl EcdsaKeyShare {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.is_empty() {
            return Err(Error::InvalidPolicy);
        }
        cb_mpc_sys::ecdsa_mp_public_key_compressed(bytes)?;
        Ok(Self(SecretBytes::new(bytes.to_vec())))
    }

    /// Export serialized private share material in a zeroizing container.
    pub fn to_bytes(&self) -> SecretBytes {
        SecretBytes::new(self.0.expose_secret().to_vec())
    }

    pub fn public_key_compressed(&self) -> Result<Vec<u8>, Error> {
        Ok(cb_mpc_sys::ecdsa_mp_public_key_compressed(
            self.0.expose_secret(),
        )?)
    }

    pub fn detach(&self) -> Result<DetachedKeyShare, Error> {
        let public_share = cb_mpc_sys::ecdsa_mp_public_share_compressed(self.0.expose_secret())?;
        let (public_key_blob, private_scalar) =
            cb_mpc_sys::ecdsa_mp_detach_private_scalar(self.0.expose_secret())?;
        Ok(DetachedKeyShare {
            public_key_blob: SecretBytes::new(public_key_blob),
            private_scalar: SecretBytes::new(private_scalar),
            public_share,
        })
    }

    pub fn attach(detached: DetachedKeyShare) -> Result<Self, Error> {
        let key_blob = cb_mpc_sys::ecdsa_mp_attach_private_scalar(
            detached.public_key_blob.expose_secret(),
            detached.private_scalar.expose_secret(),
            &detached.public_share,
        )?;
        Self::from_bytes(&key_blob)
    }
}

pub struct DetachedKeyShare {
    public_key_blob: SecretBytes,
    private_scalar: SecretBytes,
    public_share: Vec<u8>,
}

impl fmt::Debug for DetachedKeyShare {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DetachedKeyShare")
            .field("public_key_blob", &"REDACTED")
            .field("private_scalar", &"REDACTED")
            .field("public_share", &self.public_share)
            .finish()
    }
}

impl DetachedKeyShare {
    pub fn public_key_blob(&self) -> &SecretBytes {
        &self.public_key_blob
    }

    pub fn private_scalar(&self) -> &SecretBytes {
        &self.private_scalar
    }

    pub fn public_share_compressed(&self) -> &[u8] {
        &self.public_share
    }
}

pub fn dkg(
    self_index: usize,
    policy: &ThresholdPolicy,
    quorum: &[&str],
    transport: &dyn Transport,
) -> Result<(EcdsaKeyShare, SessionId), Error> {
    if self_index >= policy.party_names.len() {
        return Err(Error::InvalidParty);
    }
    let quorum = policy.validate_quorum(quorum)?;
    let (key_blob, sid) = cb_mpc_sys::ecdsa_mp_dkg_threshold(
        self_index,
        &policy.party_names,
        policy.threshold,
        &quorum,
        transport,
    )?;
    Ok((EcdsaKeyShare(SecretBytes::new(key_blob)), SessionId(sid)))
}

pub fn refresh(
    self_index: usize,
    policy: &ThresholdPolicy,
    quorum: &[&str],
    session: &SessionId,
    key_share: &EcdsaKeyShare,
    transport: &dyn Transport,
) -> Result<(EcdsaKeyShare, SessionId), Error> {
    if self_index >= policy.party_names.len() {
        return Err(Error::InvalidParty);
    }
    let quorum = policy.validate_quorum(quorum)?;
    let (key_blob, sid) = cb_mpc_sys::ecdsa_mp_refresh_threshold(
        self_index,
        &policy.party_names,
        policy.threshold,
        &quorum,
        session.as_bytes(),
        key_share.0.expose_secret(),
        transport,
    )?;
    Ok((EcdsaKeyShare(SecretBytes::new(key_blob)), SessionId(sid)))
}

pub fn sign(
    self_index: usize,
    online_party_names: &[String],
    policy: &ThresholdPolicy,
    key_share: &EcdsaKeyShare,
    message_hash: &[u8],
    signature_receiver: usize,
    transport: &dyn Transport,
) -> Result<Option<Vec<u8>>, Error> {
    if message_hash.len() != 32 {
        return Err(Error::InvalidDigestLength);
    }
    policy.validate_online_parties(online_party_names)?;
    if self_index >= online_party_names.len() || signature_receiver >= online_party_names.len() {
        return Err(Error::InvalidParty);
    }
    let signature = cb_mpc_sys::ecdsa_mp_sign_threshold(
        self_index,
        online_party_names,
        &policy.party_names,
        policy.threshold,
        key_share.0.expose_secret(),
        message_hash,
        signature_receiver,
        transport,
    )?;
    Ok((!signature.is_empty()).then_some(signature))
}

/// Safe integration point for a Catomicals signer backend.
///
/// The backend owns network/session orchestration and may use the free
/// functions above internally. Consumers never receive an FFI handle.
pub trait ThresholdSigner {
    type Error: std::error::Error + Send + Sync + 'static;

    fn public_key_compressed(&self) -> Result<Vec<u8>, Self::Error>;
    fn sign_prehash(&self, digest: [u8; 32]) -> Result<Vec<u8>, Self::Error>;
}

#[cfg(test)]
mod secret_tests {
    use super::*;

    #[test]
    fn debug_output_redacts_key_share_bytes() {
        let share = EcdsaKeyShare(SecretBytes::new(vec![222, 173, 190, 239]));
        let debug = format!("{share:?}");

        assert!(!debug.contains("222"));
        assert!(!debug.contains("173"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn debug_output_redacts_detached_private_scalar() {
        let detached = DetachedKeyShare {
            public_key_blob: SecretBytes::new(vec![1]),
            private_scalar: SecretBytes::new(vec![222, 173, 190, 239]),
            public_share: vec![2],
        };
        let debug = format!("{detached:?}");

        assert!(!debug.contains("222"));
        assert!(!debug.contains("173"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn dropping_secret_bytes_zeroizes_before_deallocation() {
        reset_secret_drop_probe();
        drop(SecretBytes::new(vec![222, 173, 190, 239]));

        assert!(last_secret_drop_was_zeroized());
    }
}
