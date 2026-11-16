use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use wireguard::x25519_dalek::{PublicKey, StaticSecret};

pub struct KeyPair {
    pub private: [u8; 32],
    pub public: [u8; 32],
}

pub fn generate() -> Result<KeyPair> {
    let mut private = [0u8; 32];
    getrandom::getrandom(&mut private)
        .map_err(|err| anyhow::anyhow!("failed to read random bytes: {err}"))?;
    let public = public_from_private(private);
    Ok(KeyPair { private, public })
}

pub fn public_from_private(private: [u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(private)).to_bytes()
}

pub fn encode(key: &[u8]) -> String {
    STANDARD.encode(key)
}

pub fn decode(value: &str) -> Result<[u8; 32]> {
    let bytes = STANDARD
        .decode(value.trim())
        .context("invalid base64 key")?;
    if bytes.len() != 32 {
        bail!("invalid key length {} (expected 32 bytes)", bytes.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_is_derived_deterministically() {
        let private = [1u8; 32];
        assert_eq!(public_from_private(private), public_from_private(private));
        assert_ne!(public_from_private(private), [0u8; 32]);
    }

    #[test]
    fn keys_round_trip_through_base64() {
        let pair = generate().unwrap();
        assert_eq!(decode(&encode(&pair.private)).unwrap(), pair.private);
        assert_eq!(decode(&encode(&pair.public)).unwrap(), pair.public);
        assert_eq!(encode(&pair.private).len(), 44);
    }

    #[test]
    fn rejects_wrong_lengths() {
        assert!(decode("AAAA").is_err());
    }
}
