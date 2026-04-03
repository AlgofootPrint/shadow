//! Key persistence — stores the stealth identity in ~/.shadow-wallet/identity.json
//!
//! Format:
//! {
//!   "spending_key": "<64 hex chars>",
//!   "viewing_key":  "<64 hex chars>"
//! }
//!
//! NOTE: keys are stored unencrypted for the hackathon prototype.
//! Production use must wrap with AES-256-GCM (same scheme OWS vault uses).

use std::path::PathBuf;
use std::fs;

use serde::{Deserialize, Serialize};

use ows_private::stealth::StealthPrivateKey;
use ows_private::PrivateError;

#[derive(Serialize, Deserialize)]
struct KeyFile {
    spending_key: String,
    viewing_key:  String,
}

/// Returns `~/.shadow-wallet/identity.json`
pub fn identity_path() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".shadow-wallet").join("identity.json")
}

/// Save a `StealthPrivateKey` to disk.
pub fn save(keys: &StealthPrivateKey) -> Result<PathBuf, String> {
    let spending = hex::encode(keys.spending_key.to_bytes());
    let viewing  = hex::encode(keys.viewing_key.to_bytes());

    let kf = KeyFile { spending_key: spending, viewing_key: viewing };
    let json = serde_json::to_string_pretty(&kf)
        .map_err(|e| format!("serialise error: {e}"))?;

    let path = identity_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create dir: {e}"))?;
    }
    fs::write(&path, json)
        .map_err(|e| format!("write error: {e}"))?;

    Ok(path)
}

/// Load a `StealthPrivateKey` from disk.
pub fn load() -> Result<StealthPrivateKey, String> {
    let path = identity_path();
    let json = fs::read_to_string(&path)
        .map_err(|_| format!("no identity found at {}\n  Run `shadow-wallet keygen` first.", path.display()))?;

    let kf: KeyFile = serde_json::from_str(&json)
        .map_err(|e| format!("parse error: {e}"))?;

    let spending = hex::decode(&kf.spending_key)
        .map_err(|e| format!("bad spending key hex: {e}"))?;
    let viewing  = hex::decode(&kf.viewing_key)
        .map_err(|e| format!("bad viewing key hex: {e}"))?;

    let spending_arr: [u8; 32] = spending.try_into()
        .map_err(|_| "spending key must be 32 bytes".to_string())?;
    let viewing_arr: [u8; 32] = viewing.try_into()
        .map_err(|_| "viewing key must be 32 bytes".to_string())?;

    StealthPrivateKey::from_bytes(&spending_arr, &viewing_arr)
        .map_err(|e: PrivateError| e.to_string())
}
