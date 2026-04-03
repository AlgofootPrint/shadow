//! Announcement scanning — the recipient side of ERC-5564.
//!
//! In production this would watch the `ERC5564Announcer` contract events.
//! For the hackathon demo we maintain an in-memory log and expose an API
//! that an indexer / RPC poller can push announcements into.

use std::collections::HashMap;

use crate::error::PrivateError;
use crate::stealth::derive::{eip55_address, recover_stealth_key, StealthAnnouncement};
use crate::stealth::keys::StealthPrivateKey;
use zeroize::Zeroizing;

/// A discovered stealth payment: the stealth address and its private key,
/// ready to spend.
#[derive(Debug)]
pub struct StealthPayment {
    /// EIP-55 checksum address
    pub address: String,
    /// Raw 20-byte address
    pub address_bytes: [u8; 20],
    /// Private key for this one-time address — zeroized on drop
    pub private_key: Zeroizing<[u8; 32]>,
}

/// Scans a stream of announcements and returns any that belong to `keys`.
pub fn scan_announcements(
    announcements: &[StealthAnnouncement],
    keys: &StealthPrivateKey,
) -> Result<Vec<StealthPayment>, PrivateError> {
    let mut found = Vec::new();

    for ann in announcements {
        if let Some(priv_key) = recover_stealth_key(ann, keys)? {
            found.push(StealthPayment {
                address: eip55_address(&ann.stealth_address),
                address_bytes: ann.stealth_address,
                private_key: priv_key,
            });
        }
    }

    Ok(found)
}

/// In-memory announcement registry — stands in for an on-chain event log.
#[derive(Default)]
pub struct AnnouncementLog {
    entries: Vec<StealthAnnouncement>,
}

impl AnnouncementLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new announcement (called after a stealth transfer is sent).
    pub fn push(&mut self, ann: StealthAnnouncement) {
        self.entries.push(ann);
    }

    /// Scan all logged announcements for payments belonging to `keys`.
    pub fn scan(&self, keys: &StealthPrivateKey) -> Result<Vec<StealthPayment>, PrivateError> {
        scan_announcements(&self.entries, keys)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Tracks which stealth addresses have already been claimed to avoid
/// double-processing in long-running agents.
#[derive(Default)]
pub struct SpentTracker {
    spent: HashMap<[u8; 20], bool>,
}

impl SpentTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_spent(&mut self, address: [u8; 20]) {
        self.spent.insert(address, true);
    }

    pub fn is_spent(&self, address: &[u8; 20]) -> bool {
        self.spent.contains_key(address)
    }

    /// Filter a list of payments to only unspent ones.
    pub fn unspent<'a>(&self, payments: &'a [StealthPayment]) -> Vec<&'a StealthPayment> {
        payments
            .iter()
            .filter(|p| !self.is_spent(&p.address_bytes))
            .collect()
    }
}
