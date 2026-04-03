//! OWS policy executable integration.
//!
//! OWS calls this binary as a subprocess before every signing operation:
//!
//!   echo '<PolicyContext JSON>' | shadow-wallet policy
//!
//! We read PolicyContext from stdin, evaluate RequireStealth against the
//! loaded identity, and write PolicyResult to stdout.
//!
//! OWS policy executable protocol:
//!   stdin  → PolicyContext JSON (one object, may be pretty-printed)
//!   stdout → PolicyResult JSON  { "allow": bool, "reason": string | null }
//!   exit 0 always (non-zero exit treated as infra error, not policy deny)
//!
//! Reference: https://docs.openwallet.sh/policy-engine#custom-executables

use std::io::{self, Read};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// OWS PolicyContext (subset we care about)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PolicyContext {
    pub chain_id:    Option<String>,
    pub api_key_id:  Option<String>,
    pub transaction: Option<TxContext>,
    pub timestamp:   Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct TxContext {
    pub to:      Option<String>,
    pub value:   Option<String>,
    pub data:    Option<String>,
    pub raw_hex: Option<String>,
}

// ---------------------------------------------------------------------------
// OWS PolicyResult
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PolicyResult {
    pub allow:  bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl PolicyResult {
    pub fn allow() -> Self { Self { allow: true, reason: None } }
    pub fn deny(reason: impl Into<String>) -> Self {
        Self { allow: false, reason: Some(reason.into()) }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Read PolicyContext from stdin, evaluate, print PolicyResult to stdout.
pub fn run() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap_or(0);
    let result = evaluate_json(&input);
    println!("{}", serde_json::to_string(&result).unwrap_or_else(|_| {
        r#"{"allow":false,"reason":"shadow-wallet internal serialise error"}"#.into()
    }));
}

/// Evaluate a PolicyContext JSON string and return a PolicyResult.
/// Exposed for use by the `demo` command.
pub fn evaluate_json(input: &str) -> PolicyResult {
    evaluate(input)
}

// ---------------------------------------------------------------------------
// Core evaluation logic
// ---------------------------------------------------------------------------

fn evaluate(input: &str) -> PolicyResult {
    // Parse context — if we can't parse, deny with explanation
    let ctx: PolicyContext = match serde_json::from_str(input) {
        Ok(c)  => c,
        Err(e) => return PolicyResult::deny(format!("shadow-wallet: failed to parse PolicyContext: {e}")),
    };

    let tx = match ctx.transaction {
        Some(ref t) => t,
        None        => return PolicyResult::deny("shadow-wallet: no transaction in PolicyContext"),
    };

    // Only evaluate on EVM chains
    if let Some(ref chain) = ctx.chain_id {
        if !chain.starts_with("eip155:") {
            return PolicyResult::allow(); // not our concern
        }
    }

    let to = match tx.to.as_deref() {
        Some(t) => t.to_lowercase(),
        None    => return PolicyResult::deny("shadow-wallet: transaction has no `to` field"),
    };

    // Check against registered stealth addresses in our keystore
    match check_is_stealth(&to) {
        Ok(true)  => PolicyResult::allow(),
        Ok(false) => PolicyResult::deny(format!(
            "RequireStealth: destination {} is not a known stealth address. \
             Derive one first with `shadow-wallet address derive --to <meta-address>`.",
            to
        )),
        Err(e) => PolicyResult::deny(format!(
            "shadow-wallet: could not load identity: {e}"
        )),
    }
}

// ---------------------------------------------------------------------------
// Stealth address registry check
// ---------------------------------------------------------------------------

/// Returns true if `addr` appears in the pending send registry
/// (~/.shadow-wallet/pending.json).
fn check_is_stealth(addr: &str) -> Result<bool, String> {
    let path = crate::keystore::identity_path()
        .parent()
        .unwrap()
        .join("pending.json");

    if !path.exists() {
        return Ok(false);
    }

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read pending.json: {e}"))?;

    let addrs: Vec<String> = serde_json::from_str(&raw)
        .map_err(|e| format!("parse pending.json: {e}"))?;

    Ok(addrs.iter().any(|a| a.to_lowercase() == addr))
}

// ---------------------------------------------------------------------------
// Public helpers called by the `send` command to pre-register a stealth addr
// ---------------------------------------------------------------------------

/// Add a stealth address to the pending registry so the policy gate passes.
pub fn register_pending(addr: &str) -> Result<(), String> {
    let path = crate::keystore::identity_path()
        .parent()
        .unwrap()
        .join("pending.json");

    let mut addrs: Vec<String> = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read pending.json: {e}"))?;
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        Vec::new()
    };

    let normalized = addr.to_lowercase();
    if !addrs.contains(&normalized) {
        addrs.push(normalized);
    }

    let json = serde_json::to_string_pretty(&addrs)
        .map_err(|e| format!("serialise: {e}"))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("write pending.json: {e}"))?;

    Ok(())
}

/// Clear a stealth address from the pending registry after broadcast.
pub fn clear_pending(addr: &str) -> Result<(), String> {
    let path = crate::keystore::identity_path()
        .parent()
        .unwrap()
        .join("pending.json");

    if !path.exists() { return Ok(()); }

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read: {e}"))?;
    let mut addrs: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
    let normalized = addr.to_lowercase();
    addrs.retain(|a| a != &normalized);

    let json = serde_json::to_string_pretty(&addrs)
        .map_err(|e| format!("serialise: {e}"))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("write: {e}"))?;

    Ok(())
}
