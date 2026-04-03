use k256::{
    elliptic_curve::sec1::ToEncodedPoint, PublicKey, SecretKey,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use zeroize::ZeroizeOnDrop;

use crate::error::PrivateError;

/// The private half of a stealth identity: spending key + viewing key.
/// Keep this secret — the spending key can move funds, the viewing key
/// can discover all incoming transactions.
#[derive(ZeroizeOnDrop)]
pub struct StealthPrivateKey {
    /// Authorises spending from stealth addresses (secp256k1 scalar)
    pub spending_key: SecretKey,
    /// Scans incoming announcements to find your funds (secp256k1 scalar)
    pub viewing_key: SecretKey,
}

impl StealthPrivateKey {
    /// Generate a fresh random stealth identity.
    pub fn generate() -> Self {
        Self {
            spending_key: SecretKey::random(&mut OsRng),
            viewing_key: SecretKey::random(&mut OsRng),
        }
    }

    /// Derive from two existing 32-byte scalars (e.g. loaded from OWS vault).
    pub fn from_bytes(spending: &[u8; 32], viewing: &[u8; 32]) -> Result<Self, PrivateError> {
        let spending_key = SecretKey::from_bytes(spending.into())
            .map_err(|_| PrivateError::InvalidKey("invalid spending key bytes".into()))?;
        let viewing_key = SecretKey::from_bytes(viewing.into())
            .map_err(|_| PrivateError::InvalidKey("invalid viewing key bytes".into()))?;
        Ok(Self { spending_key, viewing_key })
    }

    /// Export the public counterpart (safe to share).
    pub fn public(&self) -> StealthMetaAddress {
        StealthMetaAddress {
            spending_pubkey: self.spending_key.public_key(),
            viewing_pubkey: self.viewing_key.public_key(),
        }
    }
}

/// The shareable public identity: give this to anyone who wants to send you
/// a private transaction.
///
/// Encoded as `st:eth:0x<33-byte spending pubkey hex><33-byte viewing pubkey hex>`
/// per ERC-5564 compressed format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthMetaAddress {
    #[serde(
        serialize_with = "serialize_pubkey",
        deserialize_with = "deserialize_pubkey"
    )]
    pub spending_pubkey: PublicKey,
    #[serde(
        serialize_with = "serialize_pubkey",
        deserialize_with = "deserialize_pubkey"
    )]
    pub viewing_pubkey: PublicKey,
}

impl StealthMetaAddress {
    /// Canonical string form: `st:eth:0x<66-byte hex spending><66-byte hex viewing>`
    pub fn to_string(&self) -> String {
        let s = hex::encode(
            self.spending_pubkey
                .to_encoded_point(true)
                .as_bytes(),
        );
        let v = hex::encode(
            self.viewing_pubkey
                .to_encoded_point(true)
                .as_bytes(),
        );
        format!("st:eth:0x{s}{v}")
    }

    /// Parse the canonical string form.
    pub fn from_str(s: &str) -> Result<Self, PrivateError> {
        let hex_part = s
            .strip_prefix("st:eth:0x")
            .ok_or_else(|| PrivateError::ParseError("expected st:eth:0x prefix".into()))?;

        if hex_part.len() != 132 {
            return Err(PrivateError::ParseError(format!(
                "expected 132 hex chars, got {}",
                hex_part.len()
            )));
        }

        let spending_bytes = hex::decode(&hex_part[..66])
            .map_err(|e| PrivateError::ParseError(e.to_string()))?;
        let viewing_bytes = hex::decode(&hex_part[66..])
            .map_err(|e| PrivateError::ParseError(e.to_string()))?;

        let spending_pubkey = PublicKey::from_sec1_bytes(&spending_bytes)
            .map_err(|_| PrivateError::InvalidKey("invalid spending pubkey".into()))?;
        let viewing_pubkey = PublicKey::from_sec1_bytes(&viewing_bytes)
            .map_err(|_| PrivateError::InvalidKey("invalid viewing pubkey".into()))?;

        Ok(Self { spending_pubkey, viewing_pubkey })
    }
}

// --- serde helpers for k256::PublicKey ---

fn serialize_pubkey<S>(pk: &PublicKey, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let hex = hex::encode(pk.to_encoded_point(true).as_bytes());
    ser.serialize_str(&hex)
}

fn deserialize_pubkey<'de, D>(de: D) -> Result<PublicKey, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let hex_str = String::deserialize(de)?;
    let bytes = hex::decode(&hex_str).map_err(serde::de::Error::custom)?;
    PublicKey::from_sec1_bytes(&bytes).map_err(serde::de::Error::custom)
}
