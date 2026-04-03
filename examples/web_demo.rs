//! Shadow Wallet — browser live view
//!
//! Runs the full stealth + Aztec demo and streams each step to a browser
//! via Server-Sent Events.
//!
//! Start:  cargo run --example web_demo
//! Open:   http://localhost:3000

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    Router,
    response::{Html, sse::{Event, KeepAlive, Sse}},
    routing::get,
};
use tokio::sync::broadcast;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use ows_private::{
    agent::{PrivateAgent, WalletIdentity},
    aztec::AztecBridge,
    stealth::StealthPrivateKey,
};

// ---------------------------------------------------------------------------
// HTML shell
// ---------------------------------------------------------------------------

const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Shadow Wallet — live demo</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    background: #0d1117;
    color: #c9d1d9;
    font-family: 'Fira Mono', 'Cascadia Code', 'Consolas', monospace;
    font-size: 14px;
    padding: 2rem;
  }
  h1 {
    color: #58a6ff;
    font-size: 1.1rem;
    margin-bottom: 1.5rem;
    letter-spacing: 0.05em;
  }
  #output {
    white-space: pre-wrap;
    word-break: break-all;
    line-height: 1.7;
    max-width: 900px;
  }
  .sep   { color: #30363d; }
  .head  { color: #58a6ff; font-weight: bold; }
  .pass  { color: #3fb950; }
  .fail  { color: #f85149; }
  .addr  { color: #d2a8ff; }
  .key   { color: #ffa657; }
  .dim   { color: #484f58; }
  .aztec { color: #bc8cff; }
  #cursor {
    display: inline-block;
    width: 8px; height: 1em;
    background: #58a6ff;
    animation: blink 1s step-end infinite;
    vertical-align: text-bottom;
    margin-left: 2px;
  }
  @keyframes blink { 50% { opacity: 0; } }
</style>
</head>
<body>
<h1>⬡ Shadow Wallet — private agent demo</h1>
<div id="output"></div><span id="cursor"></span>
<script>
const out = document.getElementById('output');
const cursor = document.getElementById('cursor');

function classify(line) {
  if (line.startsWith('─')) return 'sep';
  if (/^\[/.test(line)) return 'head';
  if (/✓/.test(line)) return 'pass';
  if (/✗/.test(line)) return 'fail';
  if (/st:eth:|0x[0-9a-fA-F]{10}/.test(line)) return 'addr';
  if (/privkey/.test(line)) return 'key';
  if (/Sandbox|Aztec/.test(line)) return 'aztec';
  if (/^  [A-Z]/.test(line) && line.length < 60) return 'dim';
  return '';
}

const src = new EventSource('/events');
src.onmessage = (e) => {
  const line = e.data;
  const cls = classify(line);
  const span = document.createElement('span');
  if (cls) span.className = cls;
  span.textContent = line + '\n';
  out.appendChild(span);
  window.scrollTo(0, document.body.scrollHeight);
};
src.addEventListener('done', () => {
  cursor.style.display = 'none';
});
</script>
</body>
</html>"#;

// ---------------------------------------------------------------------------
// SSE handler
// ---------------------------------------------------------------------------

async fn sse_handler(
    axum::extract::State(tx): axum::extract::State<broadcast::Sender<String>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(data) => Some(Ok(Event::default().data(data))),
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// Demo
// ---------------------------------------------------------------------------

async fn run_demo(tx: broadcast::Sender<String>) {
    macro_rules! emit {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            let _ = tx.send(line);
            tokio::time::sleep(Duration::from_millis(60)).await;
        }};
    }
    macro_rules! sep {
        () => { emit!("{}", "─".repeat(60)); };
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    // ── Step 1 ──────────────────────────────────────────────────────────────
    sep!();
    emit!("[1/6] Bob generates a stealth identity");
    sep!();

    let bob_keys = StealthPrivateKey::generate();
    let bob_meta = bob_keys.public();
    emit!("  Bob's stealth meta-address:");
    emit!("  {}", bob_meta.to_string());
    emit!("");
    emit!("  Bob publishes this — anyone can send to it without linking");
    emit!("  transactions to each other or to Bob's wallet.");
    tokio::time::sleep(Duration::from_millis(400)).await;

    // ── Step 2 ──────────────────────────────────────────────────────────────
    sep!();
    emit!("[2/6] Alice sets up her PrivateAgent (max 1 ETH per tx)");
    sep!();

    let alice_wallet = WalletIdentity {
        id: "alice-wallet-01".into(),
        name: "Alice Treasury".into(),
        evm_address: "0xAlice000000000000000000000000000000000001".into(),
    };
    let alice_keys = StealthPrivateKey::generate();
    let mut alice_agent = PrivateAgent::new(alice_wallet, alice_keys)
        .with_max_value(1_000_000_000_000_000_000u128);

    emit!("  Alice's agent is live.");
    emit!("  Policies active:");
    emit!("    • RequireStealth  — only stealth addresses allowed");
    emit!("    • MaxValue(1 ETH) — no tx over 1 ETH");
    tokio::time::sleep(Duration::from_millis(400)).await;

    // ── Step 3 ──────────────────────────────────────────────────────────────
    sep!();
    emit!("[3/6] Alice prepares a stealth send to Bob");
    sep!();

    let (stealth_addr, announcement) = alice_agent
        .prepare_stealth_send(&bob_meta)
        .expect("stealth derivation failed");

    emit!("  One-time stealth address for Bob:");
    emit!("  {stealth_addr}");
    emit!("");
    emit!("  Announcement (goes on-chain via ERC5564Announcer):");
    emit!("  scheme_id        = {}", announcement.scheme_id);
    emit!("  ephemeral_pubkey = 0x{}", hex::encode(announcement.ephemeral_pubkey));
    emit!("  view_tag         = 0x{:02x}", announcement.view_tag);
    emit!("  stealth_address  = 0x{}", hex::encode(announcement.stealth_address));
    emit!("");
    emit!("  An on-chain observer sees a random address and an ephemeral key.");
    emit!("  They cannot determine who the recipient is.");
    tokio::time::sleep(Duration::from_millis(400)).await;

    // ── Step 4 ──────────────────────────────────────────────────────────────
    sep!();
    emit!("[4/6] Policy gate — Alice signs the transaction");
    sep!();

    let tx_hex = "0xf86c...";
    let value_wei: u128 = 500_000_000_000_000_000;

    match alice_agent.sign_and_send(&stealth_addr, value_wei, tx_hex) {
        Ok(tx_hash) => {
            emit!("  ✓ RequireStealth  — PASS (destination is a registered stealth addr)");
            emit!("  ✓ MaxValue(1 ETH) — PASS (0.5 ETH ≤ 1 ETH)");
            emit!("");
            emit!("  Signed tx hash: {tx_hash}");
            emit!("");
            emit!("  Transaction broadcast. Announcement published.");
        }
        Err(e) => emit!("  ✗ Policy denied: {e}"),
    }
    tokio::time::sleep(Duration::from_millis(400)).await;

    // ── Step 5 ──────────────────────────────────────────────────────────────
    sep!();
    emit!("[5/6] Policy blocks a non-stealth send");
    sep!();

    let non_stealth = "0xDEADBEEF000000000000000000000000DEADBEEF";
    let result = alice_agent.check_policies(non_stealth, Some(100), tx_hex);

    emit!("  Attempting to send to a regular address: {non_stealth}");
    if result.allow {
        emit!("  [unexpected] policy allowed — something is wrong!");
    } else {
        emit!("  ✗ Policy DENIED");
        emit!("  Reason: {}", result.reason.unwrap_or_default());
        emit!("");
        emit!("  The agent cannot sign for a traceable destination.");
    }
    tokio::time::sleep(Duration::from_millis(400)).await;

    // ── Step 6 ──────────────────────────────────────────────────────────────
    sep!();
    emit!("[6/6] Bob scans the announcement log");
    sep!();

    let bob_wallet = WalletIdentity {
        id: "bob-wallet-01".into(),
        name: "Bob Treasury".into(),
        evm_address: "0xBob0000000000000000000000000000000000001".into(),
    };
    let mut bob_agent = PrivateAgent::new(bob_wallet, bob_keys);
    bob_agent.ingest_announcement(announcement);

    match bob_agent.scan_incoming() {
        Ok(payments) if payments.is_empty() => {
            emit!("  No payments found.");
        }
        Ok(payments) => {
            emit!("  Found {} incoming stealth payment(s):", payments.len());
            for p in &payments {
                emit!("    address : {}", p.address);
                emit!("    privkey : 0x{}...  (zeroized on drop)",
                    &hex::encode(&*p.private_key)[..8]);
            }
            emit!("");
            emit!("  Bob can now spend from this one-time address.");
            emit!("  No one can link it to his identity or to Alice.");
        }
        Err(e) => emit!("  scan failed: {e}"),
    }
    tokio::time::sleep(Duration::from_millis(600)).await;

    // ── Aztec live ───────────────────────────────────────────────────────────
    sep!();
    emit!("[+] Aztec private L2 — live sandbox query");
    sep!();

    let bridge = AztecBridge::sandbox();

    match bridge.get_node_info().await {
        Ok(info) => {
            let version = info.get("nodeVersion").and_then(|v| v.as_str()).unwrap_or("?");
            let chain   = info.get("chainId").and_then(|v| v.as_u64()).unwrap_or(0);
            emit!("  Sandbox online  version={version}  chainId={chain}");
        }
        Err(e) => emit!("  Sandbox unreachable: {e}"),
    }

    match bridge.get_accounts().await {
        Ok(accounts) if !accounts.is_empty() => {
            emit!("  {} Aztec account(s) registered:", accounts.len());
            for a in &accounts {
                emit!("    {}", &a.address[..20]);
            }
            emit!("");
            emit!("  EVM layer  → stealth addresses hide the recipient");
            emit!("  Aztec L2   → ZK proofs hide the amount + sender intent");
        }
        Ok(_) => emit!("  No accounts registered."),
        Err(e) => emit!("  get_accounts failed: {e}"),
    }

    sep!();
    emit!("  Shadow Wallet demo complete.");
    sep!();

    // signal the browser that we're done
    let _ = tx.send("event: done\ndata: \n".into());
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let (tx, _rx) = broadcast::channel::<String>(256);
    let tx_demo = tx.clone();

    tokio::spawn(async move {
        run_demo(tx_demo).await;
    });

    let app = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/events", get(sse_handler))
        .with_state(tx);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Open http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}
