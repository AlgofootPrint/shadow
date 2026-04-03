//! ERC-5564 stealth address derivation.
//!
//! Protocol (SECP256K1 / scheme id = 1):
//!
//! **Sender side** (given recipient's StealthMetaAddress):
//!   1. Generate ephemeral key pair (r, R = r·G)
//!   2. Shared secret S = r · viewing_pubkey  (ECDH)
//!   3. Hash:  h = keccak256(encode(S))
//!   4. Stealth pub = spending_pubkey + h·G
//!   5. Stealth address = keccak256(stealth_pub_uncompressed[1..])[12..]  (EIP-55)
//!   6. Announce: (scheme=1, ephemeral_pubkey=R, view_tag=h[0])
//!
//! **Recipient side** (given announcement + StealthPrivateKey):
//!   1. S = viewing_key · ephemeral_pubkey  (ECDH — same shared secret)
//!   2. h = keccak256(encode(S))
//!   3. Quick check: h[0] == view_tag (skip 255/256 of non-matching announcements)
//!   4. stealth_priv = spending_key + h  (scalar addition mod n)
//!   5. Derive stealth address from stealth_priv → check against on-chain

use k256::{
    elliptic_curve::{
        ops::Reduce,
        sec1::ToEncodedPoint,
        PrimeField,
    },
    NonZeroScalar, ProjectivePoint, PublicKey, Scalar, SecretKey, U256,
};
use rand::rngs::OsRng;
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::error::PrivateError;
use crate::stealth::keys::{StealthMetaAddress, StealthPrivateKey};

/// Everything the sender puts on-chain / passes to the recipient so they
/// can discover and spend the funds.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StealthAnnouncement {
    /// ERC-5564 scheme identifier (1 = secp256k1)
    pub scheme_id: u8,
    /// Compressed ephemeral public key R (33 bytes), hex-encoded
    #[serde(with = "hex_33")]
    pub ephemeral_pubkey: [u8; 33],
    /// First byte of h — lets recipients skip 255/256 irrelevant announcements
    pub view_tag: u8,
    /// The derived stealth address (20 bytes, EVM), hex-encoded
    #[serde(with = "hex_20")]
    pub stealth_address: [u8; 20],
}

mod hex_33 {
    use serde::{Deserializer, Serializer, Deserialize};
    pub fn serialize<S: Serializer>(v: &[u8; 33], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 33], D::Error> {
        let h = String::deserialize(d)?;
        let b = hex::decode(&h).map_err(serde::de::Error::custom)?;
        b.try_into().map_err(|_| serde::de::Error::custom("expected 33 bytes"))
    }
}

mod hex_20 {
    use serde::{Deserializer, Serializer, Deserialize};
    pub fn serialize<S: Serializer>(v: &[u8; 20], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 20], D::Error> {
        let h = String::deserialize(d)?;
        let b = hex::decode(&h).map_err(serde::de::Error::custom)?;
        b.try_into().map_err(|_| serde::de::Error::custom("expected 20 bytes"))
    }
}

/// Derive a stealth address for `recipient` and produce the announcement.
/// Call this on the **sender** side before constructing the transaction.
pub fn derive_stealth(
    recipient: &StealthMetaAddress,
) -> Result<StealthAnnouncement, PrivateError> {
    // 1. Ephemeral keypair
    let ephemeral_secret = SecretKey::random(&mut OsRng);
    let ephemeral_pubkey = ephemeral_secret.public_key();

    // 2. ECDH shared point  S = r · V_pub  (manual scalar mult)
    let ephemeral_scalar = NonZeroScalar::from_repr(ephemeral_secret.to_bytes())
        .into_option()
        .ok_or_else(|| PrivateError::InvalidKey("ephemeral key is zero".into()))?;
    let viewing_point = ProjectivePoint::from(recipient.viewing_pubkey.as_affine());
    let shared_point = viewing_point * *ephemeral_scalar;
    let shared_affine = shared_point.to_affine();

    // x-coordinate of shared point
    let shared_encoded = shared_affine.to_encoded_point(false);
    let shared_x_bytes = &shared_encoded.as_bytes()[1..33];

    // 3. h = keccak256(S_x)  (per ERC-5564 secp256k1 scheme)
    let h = Keccak256::digest(shared_x_bytes);

    // 4. Stealth pubkey = K_spend + h·G
    let h_scalar = Scalar::reduce(U256::from_be_slice(&h));
    let spend_point =
        ProjectivePoint::from(recipient.spending_pubkey.as_affine());
    let stealth_point = spend_point + (ProjectivePoint::GENERATOR * h_scalar);
    let stealth_affine = stealth_point.to_affine();

    // 5. EVM address = keccak256(uncompressed[1..])[12..]
    let uncompressed = stealth_affine.to_encoded_point(false);
    let addr_hash = Keccak256::digest(&uncompressed.as_bytes()[1..]);
    let mut stealth_address = [0u8; 20];
    stealth_address.copy_from_slice(&addr_hash[12..]);

    // Pack ephemeral pubkey
    let ep_compressed = ephemeral_pubkey.to_encoded_point(true);
    let mut ephemeral_buf = [0u8; 33];
    ephemeral_buf.copy_from_slice(ep_compressed.as_bytes());

    Ok(StealthAnnouncement {
        scheme_id: 1,
        ephemeral_pubkey: ephemeral_buf,
        view_tag: h[0],
        stealth_address,
    })
}

/// On the **recipient** side: given an announcement and our private keys,
/// recover the private key for the stealth address (if it's ours).
///
/// Returns `None` if this announcement doesn't belong to us.
pub fn recover_stealth_key(
    announcement: &StealthAnnouncement,
    keys: &StealthPrivateKey,
) -> Result<Option<Zeroizing<[u8; 32]>>, PrivateError> {
    // 1. Parse ephemeral pubkey
    let ephemeral_pubkey = PublicKey::from_sec1_bytes(&announcement.ephemeral_pubkey)
        .map_err(|_| PrivateError::InvalidKey("bad ephemeral pubkey in announcement".into()))?;

    // 2. ECDH: S = viewing_key · R  (manual scalar mult)
    let viewing_scalar = NonZeroScalar::from_repr(
        keys.viewing_key.to_bytes(),
    )
    .into_option()
    .ok_or_else(|| PrivateError::InvalidKey("viewing key is zero scalar".into()))?;

    let ephemeral_point = ProjectivePoint::from(ephemeral_pubkey.as_affine());
    let shared_point = ephemeral_point * *viewing_scalar;
    let shared_affine = shared_point.to_affine();

    // x-coordinate of shared point (32 bytes)
    let shared_x = shared_affine.to_encoded_point(false);
    let shared_x_bytes = &shared_x.as_bytes()[1..33]; // skip 0x04 prefix, take x

    // 3. h = keccak256(S_x)
    let h = Keccak256::digest(shared_x_bytes);

    // 4. View tag check — skip early if first byte doesn't match
    if h[0] != announcement.view_tag {
        return Ok(None);
    }

    // 5. stealth_priv = spending_key + h  (mod n)
    let h_scalar = Scalar::reduce(U256::from_be_slice(&h));
    let spending_scalar = NonZeroScalar::from_repr(keys.spending_key.to_bytes())
        .into_option()
        .ok_or_else(|| PrivateError::InvalidKey("spending key is zero scalar".into()))?;

    let stealth_scalar = *spending_scalar + h_scalar;

    // 6. Verify the derived address matches the announcement
    let stealth_point = ProjectivePoint::GENERATOR * stealth_scalar;
    let stealth_affine = stealth_point.to_affine();
    let uncompressed = stealth_affine.to_encoded_point(false);
    let addr_hash = Keccak256::digest(&uncompressed.as_bytes()[1..]);
    let mut derived_address = [0u8; 20];
    derived_address.copy_from_slice(&addr_hash[12..]);

    if derived_address != announcement.stealth_address {
        return Ok(None); // not ours
    }

    // 7. Export stealth private key bytes
    let mut key_bytes = Zeroizing::new([0u8; 32]);
    let repr = stealth_scalar.to_repr();
    key_bytes.copy_from_slice(&repr);

    Ok(Some(key_bytes))
}

/// Format a 20-byte EVM address as an EIP-55 checksum hex string.
pub fn eip55_address(addr: &[u8; 20]) -> String {
    let hex_lower = hex::encode(addr);
    let hash = Keccak256::digest(hex_lower.as_bytes());
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (i, c) in hex_lower.chars().enumerate() {
        if c.is_ascii_alphabetic() {
            let nibble = (hash[i / 2] >> (if i % 2 == 0 { 4 } else { 0 })) & 0xf;
            if nibble >= 8 {
                out.push(c.to_ascii_uppercase());
            } else {
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}
