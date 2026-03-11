//! Plugin signing and verification.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use super::capabilities::Capabilities;
use super::limits::ResourceLimits;

/// Plugin signature for verification.
#[derive(Clone, Debug)]
pub struct PluginSignature {
    /// Ed25519 signature.
    pub signature: Signature,
    /// Public key that created the signature.
    pub public_key: VerifyingKey,
    /// Hash of the signed content.
    pub content_hash: [u8; 32],
}

impl PluginSignature {
    /// Sign plugin bytes.
    pub fn sign(bytes: &[u8], signing_key: &SigningKey) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let content_hash: [u8; 32] = hasher.finalize().into();

        let signature = signing_key.sign(&content_hash);
        let public_key = signing_key.verifying_key();

        Self {
            signature,
            public_key,
            content_hash,
        }
    }

    /// Verify signature against bytes.
    pub fn verify(&self, bytes: &[u8]) -> bool {
        // Check content hash
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let computed_hash: [u8; 32] = hasher.finalize().into();

        if computed_hash != self.content_hash {
            return false;
        }

        // Verify signature
        self.public_key
            .verify(&self.content_hash, &self.signature)
            .is_ok()
    }
}

/// Trusted public keys for plugin verification.
#[derive(Clone, Debug, Default)]
pub struct TrustedKeys {
    keys: Vec<VerifyingKey>,
}

impl TrustedKeys {
    /// Create a new empty trusted keys set.
    pub fn new() -> Self {
        Self { keys: Vec::new() }
    }

    /// Add a trusted key.
    pub fn add(&mut self, key: VerifyingKey) {
        self.keys.push(key);
    }

    /// Check if a signature is from a trusted key.
    pub fn is_trusted(&self, signature: &PluginSignature) -> bool {
        self.keys.iter().any(|k| k == &signature.public_key)
    }

    /// Get the number of trusted keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Check if there are no trusted keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Trust level for a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustLevel {
    /// Plugin is signed by a trusted key.
    Trusted,
    /// Plugin is signed but key is not trusted.
    Signed,
    /// Plugin is not signed.
    Unsigned,
}

/// Determine trust level for a plugin.
pub fn determine_trust_level(
    bytes: &[u8],
    signature: Option<&PluginSignature>,
    trusted_keys: &TrustedKeys,
) -> TrustLevel {
    match signature {
        Some(sig) if sig.verify(bytes) => {
            if trusted_keys.is_trusted(sig) {
                TrustLevel::Trusted
            } else {
                TrustLevel::Signed
            }
        }
        Some(_) => TrustLevel::Unsigned, // Invalid signature = unsigned
        None => TrustLevel::Unsigned,
    }
}

/// Get capabilities based on trust level.
pub fn capabilities_for_trust_level(level: TrustLevel) -> Capabilities {
    match level {
        TrustLevel::Trusted => Capabilities::full(),
        TrustLevel::Signed => Capabilities::standard(),
        TrustLevel::Unsigned => Capabilities::minimal(),
    }
}

/// Get limits based on trust level.
pub fn limits_for_trust_level(level: TrustLevel) -> ResourceLimits {
    match level {
        TrustLevel::Trusted => ResourceLimits::generous(),
        TrustLevel::Signed => ResourceLimits::default(),
        TrustLevel::Unsigned => ResourceLimits::restrictive(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn create_test_key() -> SigningKey {
        let mut csprng = rand::rngs::OsRng;
        SigningKey::generate(&mut csprng)
    }

    #[test]
    fn sign_and_verify() {
        let key = create_test_key();
        let data = b"test plugin data";

        let signature = PluginSignature::sign(data, &key);
        assert!(signature.verify(data));
    }

    #[test]
    fn verify_fails_on_tampered_data() {
        let key = create_test_key();
        let data = b"test plugin data";

        let signature = PluginSignature::sign(data, &key);
        assert!(!signature.verify(b"tampered data"));
    }

    #[test]
    fn trust_level_trusted() {
        let key = create_test_key();
        let data = b"test plugin data";

        let mut trusted_keys = TrustedKeys::new();
        trusted_keys.add(key.verifying_key());

        let signature = PluginSignature::sign(data, &key);
        let level = determine_trust_level(data, Some(&signature), &trusted_keys);

        assert_eq!(level, TrustLevel::Trusted);
    }

    #[test]
    fn trust_level_signed() {
        let key = create_test_key();
        let data = b"test plugin data";

        let trusted_keys = TrustedKeys::new(); // Empty - no trusted keys

        let signature = PluginSignature::sign(data, &key);
        let level = determine_trust_level(data, Some(&signature), &trusted_keys);

        assert_eq!(level, TrustLevel::Signed);
    }

    #[test]
    fn trust_level_unsigned() {
        let trusted_keys = TrustedKeys::new();
        let level = determine_trust_level(b"data", None, &trusted_keys);

        assert_eq!(level, TrustLevel::Unsigned);
    }

    #[test]
    fn capabilities_match_trust_level() {
        let full = capabilities_for_trust_level(TrustLevel::Trusted);
        let standard = capabilities_for_trust_level(TrustLevel::Signed);
        let minimal = capabilities_for_trust_level(TrustLevel::Unsigned);

        // Full should allow process spawn
        assert!(full.process.spawn);
        // Standard should not allow process spawn
        assert!(!standard.process.spawn);
        // Minimal should not allow anything
        assert!(!minimal.network.connect);
    }
}
