//! Privacy-aware policy rules that plug into the OWS policy engine model.
//!
//! These are standalone evaluators — they consume the same `PolicyContext`
//! shape as OWS and return a compatible `PolicyResult`.  When OWS exposes
//! its policy trait externally you can register them directly; until then
//! they run as the executable policy hook.

use serde::{Deserialize, Serialize};

use crate::error::PrivateError;
use crate::stealth::keys::StealthMetaAddress;

// ---------------------------------------------------------------------------
// Mirror of OWS policy types (kept local to avoid coupling)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyContext {
    pub chain_id: String,
    pub wallet_id: String,
    pub api_key_id: String,
    pub transaction: TransactionContext,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionContext {
    /// Destination address of the transaction.
    pub to: Option<String>,
    pub value: Option<String>,
    pub raw_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyResult {
    pub allow: bool,
    pub reason: Option<String>,
    pub policy_id: Option<String>,
}

impl PolicyResult {
    pub fn allow() -> Self {
        Self { allow: true, reason: None, policy_id: None }
    }

    pub fn deny(policy_id: &str, reason: impl Into<String>) -> Self {
        Self {
            allow: false,
            reason: Some(reason.into()),
            policy_id: Some(policy_id.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// RequireStealth policy rule
// ---------------------------------------------------------------------------

/// Blocks any transaction whose `to` address is NOT a known stealth address.
///
/// In a full integration this rule would be registered with the OWS policy
/// engine. Here it is a standalone evaluator you call before signing.
///
/// # How it works
/// Every time a stealth send is prepared, the derived stealth address is
/// registered with this policy via `register_stealth_address`. The rule then
/// verifies that `context.transaction.to` matches a registered address.
///
/// This ensures the agent CANNOT accidentally send to a traceable wallet.
#[derive(Debug, Default)]
pub struct RequireStealthPolicy {
    id: String,
    /// Lowercase hex stealth addresses that are cleared to receive funds.
    approved: std::collections::HashSet<String>,
}

impl RequireStealthPolicy {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), approved: Default::default() }
    }

    /// Whitelist a stealth address before the transaction is signed.
    pub fn register_stealth_address(&mut self, addr: &str) {
        self.approved.insert(addr.to_lowercase());
    }

    /// Clear all registered stealth addresses (e.g. after they're spent).
    pub fn clear(&mut self) {
        self.approved.clear();
    }

    /// Evaluate: returns `allow` only when `to` is a registered stealth addr.
    pub fn evaluate(&self, ctx: &PolicyContext) -> PolicyResult {
        let to = match &ctx.transaction.to {
            Some(addr) => addr.to_lowercase(),
            None => {
                return PolicyResult::deny(
                    &self.id,
                    "transaction has no destination — contract deployment blocked by RequireStealth",
                );
            }
        };

        if self.approved.contains(&to) {
            PolicyResult::allow()
        } else {
            PolicyResult::deny(
                &self.id,
                format!(
                    "destination {to} is not a registered stealth address; \
                     use PrivateAgent::prepare_stealth_send() to derive one first"
                ),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// MaxTransactionValue policy rule (bonus — open issue #169)
// ---------------------------------------------------------------------------

/// Denies transactions whose ETH value exceeds a threshold (in wei, as a
/// decimal string to avoid u128 overflow in JSON).
#[derive(Debug, Clone)]
pub struct MaxTransactionValue {
    pub id: String,
    /// Maximum value in wei.
    pub max_wei: u128,
}

impl MaxTransactionValue {
    pub fn new(id: impl Into<String>, max_wei: u128) -> Self {
        Self { id: id.into(), max_wei }
    }

    pub fn evaluate(&self, ctx: &PolicyContext) -> PolicyResult {
        let value_str = match &ctx.transaction.value {
            Some(v) => v.clone(),
            None => return PolicyResult::allow(), // no value field → 0 ETH
        };

        let value: u128 = match value_str.parse() {
            Ok(v) => v,
            Err(_) => {
                return PolicyResult::deny(
                    &self.id,
                    format!("cannot parse transaction value: {value_str}"),
                );
            }
        };

        if value <= self.max_wei {
            PolicyResult::allow()
        } else {
            PolicyResult::deny(
                &self.id,
                format!(
                    "transaction value {value} wei exceeds max {} wei",
                    self.max_wei
                ),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Policy chain: evaluate multiple rules with AND semantics
// ---------------------------------------------------------------------------

pub fn evaluate_all(
    rules: &[&dyn Fn(&PolicyContext) -> PolicyResult],
    ctx: &PolicyContext,
) -> PolicyResult {
    for rule in rules {
        let result = rule(ctx);
        if !result.allow {
            return result;
        }
    }
    PolicyResult::allow()
}
