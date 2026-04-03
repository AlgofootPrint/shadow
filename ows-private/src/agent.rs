//! PrivateAgent — the top-level orchestrator.
//!
//! Wraps an OWS-style wallet identity with stealth address support and
//! privacy-enforcing policies.  In a full integration this would call into
//! `ows-lib` for vault operations; here we carry a mock wallet so the demo
//! compiles and runs standalone (without requiring the OWS binary).

use crate::error::PrivateError;
use crate::policy::{MaxTransactionValue, PolicyContext, PolicyResult, RequireStealthPolicy, TransactionContext};
use crate::stealth::{
    derive_stealth, eip55_address, AnnouncementLog, SpentTracker, StealthAnnouncement,
    StealthMetaAddress, StealthPayment, StealthPrivateKey,
};

// ---------------------------------------------------------------------------
// WalletIdentity — stand-in for an OWS wallet reference
// ---------------------------------------------------------------------------

/// Minimal wallet identity.  In production this is an OWS wallet loaded from
/// the encrypted vault (`~/.ows/wallets/<id>.json`).
#[derive(Debug, Clone)]
pub struct WalletIdentity {
    pub id: String,
    pub name: String,
    /// EVM address of the hot wallet (used as fallback / gas payer)
    pub evm_address: String,
}

// ---------------------------------------------------------------------------
// PrivateAgent
// ---------------------------------------------------------------------------

/// An autonomous agent that can only interact with the chain through
/// privacy-preserving stealth addresses.
///
/// Workflow:
/// 1. Create agent: `PrivateAgent::new(wallet, stealth_keys)`
/// 2. **Send**:   `agent.prepare_stealth_send(recipient_meta)` → get stealth addr + announcement
/// 3. **Receive**: `agent.scan_incoming()` → find payments addressed to us
/// 4. **Spend**:  `agent.spend(payment)` → sign & broadcast (mocked here)
pub struct PrivateAgent {
    pub wallet: WalletIdentity,
    pub stealth_keys: StealthPrivateKey,
    pub meta_address: StealthMetaAddress,
    policy: RequireStealthPolicy,
    value_policy: Option<MaxTransactionValue>,
    log: AnnouncementLog,
    spent: SpentTracker,
}

impl PrivateAgent {
    pub fn new(wallet: WalletIdentity, stealth_keys: StealthPrivateKey) -> Self {
        let meta_address = stealth_keys.public();
        Self {
            wallet,
            stealth_keys,
            meta_address,
            policy: RequireStealthPolicy::new("require-stealth-v1"),
            value_policy: None,
            log: AnnouncementLog::new(),
            spent: SpentTracker::new(),
        }
    }

    /// Optionally cap the value of any outgoing transaction.
    pub fn with_max_value(mut self, max_wei: u128) -> Self {
        self.value_policy = Some(MaxTransactionValue::new("max-value-v1", max_wei));
        self
    }

    // -----------------------------------------------------------------------
    // Sending side
    // -----------------------------------------------------------------------

    /// Derive a one-time stealth address for `recipient` and register it with
    /// the RequireStealth policy so the subsequent `sign_transaction` call is
    /// allowed through.
    ///
    /// Returns the stealth address (EIP-55) and the announcement to publish.
    pub fn prepare_stealth_send(
        &mut self,
        recipient: &StealthMetaAddress,
    ) -> Result<(String, StealthAnnouncement), PrivateError> {
        let announcement = derive_stealth(recipient)?;
        let addr = eip55_address(&announcement.stealth_address);
        // Whitelist this address so the policy gate lets us sign
        self.policy.register_stealth_address(&addr);
        Ok((addr, announcement))
    }

    /// Check all active policies before signing.  Returns `PolicyResult`.
    pub fn check_policies(
        &self,
        to: &str,
        value_wei: Option<u128>,
        raw_hex: &str,
    ) -> PolicyResult {
        let ctx = PolicyContext {
            chain_id: "eip155:1".into(),
            wallet_id: self.wallet.id.clone(),
            api_key_id: "agent-key-0".into(),
            transaction: TransactionContext {
                to: Some(to.to_string()),
                value: value_wei.map(|v| v.to_string()),
                raw_hex: raw_hex.to_string(),
            },
            timestamp: chrono_now(),
        };

        // 1. RequireStealth
        let r = self.policy.evaluate(&ctx);
        if !r.allow {
            return r;
        }

        // 2. MaxTransactionValue (optional)
        if let Some(ref vp) = self.value_policy {
            let r2 = vp.evaluate(&ctx);
            if !r2.allow {
                return r2;
            }
        }

        PolicyResult::allow()
    }

    /// Mock sign-and-send.  In production this calls `ows_lib::sign_transaction`
    /// and then broadcasts via the chain's RPC.
    pub fn sign_and_send(
        &mut self,
        to: &str,
        value_wei: u128,
        raw_hex: &str,
    ) -> Result<String, PrivateError> {
        let policy_result = self.check_policies(to, Some(value_wei), raw_hex);
        if !policy_result.allow {
            return Err(PrivateError::PolicyDenied(
                policy_result.reason.unwrap_or_default(),
            ));
        }

        // Simulate a tx hash (in production: call ows_lib + broadcast)
        let mock_hash = format!(
            "0x{:064x}",
            value_wei ^ u128::from_le_bytes(to.as_bytes().get(..16).map_or([0u8; 16], |b| {
                let mut arr = [0u8; 16];
                arr[..b.len().min(16)].copy_from_slice(&b[..b.len().min(16)]);
                arr
            }))
        );

        // After sending, clear the one-time whitelist entry
        self.policy.clear();

        Ok(mock_hash)
    }

    // -----------------------------------------------------------------------
    // Receiving side
    // -----------------------------------------------------------------------

    /// Register an announcement from the on-chain event log.
    pub fn ingest_announcement(&mut self, ann: StealthAnnouncement) {
        self.log.push(ann);
    }

    /// Scan the announcement log for payments addressed to this agent.
    pub fn scan_incoming(&self) -> Result<Vec<StealthPayment>, PrivateError> {
        let all = self.log.scan(&self.stealth_keys)?;
        let unspent_addrs: Vec<[u8; 20]> = self
            .spent
            .unspent(&all)
            .iter()
            .map(|p| p.address_bytes)
            .collect();
        Ok(all
            .into_iter()
            .filter(|p| unspent_addrs.contains(&p.address_bytes))
            .collect())
    }

    /// Mark a stealth address as spent after broadcasting the spend tx.
    pub fn mark_spent(&mut self, address_bytes: [u8; 20]) {
        self.spent.mark_spent(address_bytes);
    }
}

// ---------------------------------------------------------------------------
// Tiny time helper (no chrono dep — just return a fixed ISO string for demo)
// ---------------------------------------------------------------------------
fn chrono_now() -> String {
    // In production: use chrono::Utc::now().to_rfc3339()
    "2026-04-03T00:00:00Z".to_string()
}
