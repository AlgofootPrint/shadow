//! Shadow Wallet — end-to-end demo
//!
//! Simulates a full private transfer flow:
//!
//!   Alice (sender)   →   Bob (recipient)
//!
//!   1. Bob generates a StealthMetaAddress and shares it publicly
//!   2. Alice derives a one-time stealth address for Bob via ECDH
//!   3. Alice's agent checks RequireStealth + MaxValue policies before signing
//!   4. Alice broadcasts the transaction + publishes the announcement
//!   5. Bob scans the announcement log → finds his payment
//!   6. Policy blocks Alice from sending to a non-stealth address
//!   7. Aztec bridge would handle fully private on-chain execution (shown as stub)

use ows_private::{
    agent::{PrivateAgent, WalletIdentity},
    aztec::AztecBridge,
    stealth::StealthPrivateKey,
};

fn separator() {
    println!("{}", "─".repeat(60));
}

#[tokio::main]
async fn main() {
    println!();
    println!("  ███████╗██╗  ██╗ █████╗ ██████╗  ██████╗ ██╗    ██╗");
    println!("  ██╔════╝██║  ██║██╔══██╗██╔══██╗██╔═══██╗██║    ██║");
    println!("  ███████╗███████║███████║██║  ██║██║   ██║██║ █╗ ██║");
    println!("  ╚════██║██╔══██║██╔══██║██║  ██║██║   ██║██║███╗██║");
    println!("  ███████║██║  ██║██║  ██║██████╔╝╚██████╔╝╚███╔███╔╝");
    println!("  ╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝  ╚══╝╚══╝ ");
    println!();
    println!("  Private Agent Wallet — built on Open Wallet Standard");
    println!();

    // -----------------------------------------------------------------------
    // Step 1: Bob generates his stealth identity
    // -----------------------------------------------------------------------
    separator();
    println!("[1/6] Bob generates a stealth identity");
    separator();

    let bob_keys = StealthPrivateKey::generate();
    let bob_meta = bob_keys.public();
    println!("  Bob's stealth meta-address:");
    println!("  {}", bob_meta.to_string());
    println!();
    println!("  Bob publishes this — anyone can send to it without linking");
    println!("  transactions to each other or to Bob's wallet.");

    // -----------------------------------------------------------------------
    // Step 2: Alice sets up her private agent
    // -----------------------------------------------------------------------
    separator();
    println!("[2/6] Alice sets up her PrivateAgent (max 1 ETH per tx)");
    separator();

    let alice_wallet = WalletIdentity {
        id: "alice-wallet-01".into(),
        name: "Alice Treasury".into(),
        evm_address: "0xAlice000000000000000000000000000000000001".into(),
    };
    let alice_keys = StealthPrivateKey::generate();
    let mut alice_agent = PrivateAgent::new(alice_wallet, alice_keys)
        .with_max_value(1_000_000_000_000_000_000u128); // 1 ETH in wei

    println!("  Alice's agent is live.");
    println!("  Policies active:");
    println!("    • RequireStealth  — only stealth addresses allowed");
    println!("    • MaxValue(1 ETH) — no tx over 1 ETH");

    // -----------------------------------------------------------------------
    // Step 3: Alice derives a stealth address for Bob
    // -----------------------------------------------------------------------
    separator();
    println!("[3/6] Alice prepares a stealth send to Bob");
    separator();

    let (stealth_addr, announcement) = alice_agent
        .prepare_stealth_send(&bob_meta)
        .expect("stealth derivation failed");

    println!("  One-time stealth address for Bob:");
    println!("  {stealth_addr}");
    println!();
    println!("  Announcement (goes on-chain via ERC5564Announcer):");
    println!("  scheme_id       = {}", announcement.scheme_id);
    println!(
        "  ephemeral_pubkey = 0x{}",
        hex::encode(announcement.ephemeral_pubkey)
    );
    println!("  view_tag        = 0x{:02x}", announcement.view_tag);
    println!(
        "  stealth_address  = 0x{}",
        hex::encode(announcement.stealth_address)
    );
    println!();
    println!("  An on-chain observer sees a random address and an ephemeral key.");
    println!("  They cannot determine who the recipient is.");

    // -----------------------------------------------------------------------
    // Step 4: Policy check + sign
    // -----------------------------------------------------------------------
    separator();
    println!("[4/6] Policy gate — Alice signs the transaction");
    separator();

    let tx_hex = "0xf86c..."; // placeholder raw tx bytes
    let value_wei: u128 = 500_000_000_000_000_000; // 0.5 ETH

    match alice_agent.sign_and_send(&stealth_addr, value_wei, tx_hex) {
        Ok(tx_hash) => {
            println!("  ✓ RequireStealth  — PASS (destination is a registered stealth addr)");
            println!("  ✓ MaxValue(1 ETH) — PASS (0.5 ETH ≤ 1 ETH)");
            println!();
            println!("  Signed tx hash: {tx_hash}");
            println!();
            println!("  Transaction broadcast. Announcement published.");
        }
        Err(e) => {
            eprintln!("  ✗ Policy denied: {e}");
            std::process::exit(1);
        }
    }

    // -----------------------------------------------------------------------
    // Step 5: Policy blocks a non-stealth send
    // -----------------------------------------------------------------------
    separator();
    println!("[5/6] Policy blocks a non-stealth send");
    separator();

    let non_stealth_addr = "0xDEADBEEF000000000000000000000000DEADBEEF";
    let result = alice_agent.check_policies(non_stealth_addr, Some(100), tx_hex);

    println!("  Attempting to send to a regular address: {non_stealth_addr}");
    if result.allow {
        println!("  [unexpected] policy allowed — something is wrong!");
    } else {
        println!("  ✗ Policy DENIED");
        println!("  Reason: {}", result.reason.unwrap_or_default());
        println!();
        println!("  The agent cannot sign for a traceable destination.");
    }

    // -----------------------------------------------------------------------
    // Step 6: Bob scans and discovers his payment
    // -----------------------------------------------------------------------
    separator();
    println!("[6/6] Bob scans the announcement log");
    separator();

    // Bob's agent ingests the announcement (from on-chain event indexer)
    let bob_wallet = WalletIdentity {
        id: "bob-wallet-01".into(),
        name: "Bob Treasury".into(),
        evm_address: "0xBob0000000000000000000000000000000000001".into(),
    };
    let mut bob_agent = PrivateAgent::new(bob_wallet, bob_keys);
    bob_agent.ingest_announcement(announcement);

    let payments = bob_agent.scan_incoming().expect("scan failed");
    if payments.is_empty() {
        println!("  No payments found (this shouldn't happen in the demo).");
    } else {
        println!("  Found {} incoming stealth payment(s):", payments.len());
        for p in &payments {
            println!("    address : {}", p.address);
            println!("    privkey : 0x{}...  (zeroized on drop)",
                &hex::encode(&*p.private_key)[..8]
            );
        }
        println!();
        println!("  Bob can now spend from this one-time address.");
        println!("  No one can link it to his identity or to Alice.");
    }

    // -----------------------------------------------------------------------
    // Aztec bridge — live sandbox call
    // -----------------------------------------------------------------------
    separator();
    println!("[+] Aztec private L2 — live sandbox query");
    separator();

    let bridge = AztecBridge::sandbox();

    match bridge.get_node_info().await {
        Ok(info) => {
            let version = info.get("nodeVersion").and_then(|v| v.as_str()).unwrap_or("?");
            let chain   = info.get("chainId").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("  Sandbox online  version={version}  chainId={chain}");
        }
        Err(e) => println!("  Sandbox unreachable: {e}  (start with: docker compose up -d)"),
    }

    match bridge.get_accounts().await {
        Ok(accounts) if accounts.is_empty() => {
            println!("  No accounts registered in sandbox.");
        }
        Ok(accounts) => {
            println!("  {} Aztec account(s) registered:", accounts.len());
            for a in &accounts {
                println!("    {}", &a.address[..18]);
            }
            println!();
            println!("  On Aztec: amount + recipient are hidden at the protocol level.");
            println!("  Combined with ERC-5564 stealth addresses, the full trail is broken:");
            println!("    EVM layer  → stealth addresses hide the recipient");
            println!("    Aztec L2   → ZK proofs hide the amount + sender intent");
        }
        Err(e) => println!("  get_accounts failed: {e}"),
    }

    separator();
    println!("  Shadow Wallet demo complete.");
    separator();
    println!();
}
