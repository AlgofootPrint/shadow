//! shadow-wallet — private stealth address layer for OWS
//!
//! USAGE
//!   shadow-wallet keygen                          Generate & save stealth identity
//!   shadow-wallet address                         Print your StealthMetaAddress
//!   shadow-wallet derive --to <meta>              Derive a one-time stealth address
//!   shadow-wallet scan [--announcements <file>]   Scan for incoming payments
//!   shadow-wallet send  --to <meta> --value <wei> Full stealth send via OWS
//!                       --wallet-id <id> [--chain-id eip155:1]
//!                       [--api-key <key>] [--ows-url <url>]
//!   shadow-wallet policy                          OWS policy executable (stdin/stdout)
//!   shadow-wallet config                          Print OWS config snippet

mod evm;
mod keystore;
mod mock_ows;
mod ows_client;
mod policy_exec;

use alloy::primitives::{Address as EvmAddress, U256};
use clap::{Parser, Subcommand};
use ows_private::{
    aztec::AztecBridge,
    stealth::{StealthMetaAddress, StealthPrivateKey},
    AnnouncementLog, StealthAnnouncement,
};
use ows_client::{OwsClient, SignAndSendRequest};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "shadow-wallet",
    about = "Private stealth address layer for OWS — ERC-5564 + Aztec",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a new stealth identity and save to ~/.shadow-wallet/identity.json
    Keygen {
        /// Overwrite existing identity without prompting
        #[arg(long)]
        force: bool,
    },

    /// Print your StealthMetaAddress (share this to receive private payments)
    Address,

    /// Derive a one-time stealth address for a recipient
    Derive {
        /// Recipient's StealthMetaAddress (st:eth:0x...)
        #[arg(long)]
        to: String,
    },

    /// Scan announcements for incoming stealth payments
    Scan {
        /// JSON file of StealthAnnouncement objects (omit to read from stdin)
        #[arg(long)]
        announcements: Option<String>,
    },

    /// Derive a stealth address and send ETH
    Send {
        /// Recipient's StealthMetaAddress
        #[arg(long)]
        to: String,

        /// Amount in wei (e.g. 1000000000000000 = 0.001 ETH)
        #[arg(long)]
        value: String,

        // ── Real EVM mode (use these for real/testnet funds) ─────────────────

        /// Ethereum RPC URL — enables real on-chain sending
        /// e.g. https://rpc.sepolia.org  or  https://sepolia.infura.io/v3/<KEY>
        #[arg(long)]
        rpc_url: Option<String>,

        /// Sender's private key (0x...) — or set SHADOW_WALLET_KEY env var
        /// Only used with --rpc-url. Never logged or stored.
        #[arg(long, env = "SHADOW_WALLET_KEY")]
        private_key: Option<String>,

        /// Also call the ERC-5564 Announcer contract so recipients can scan
        #[arg(long)]
        announce: bool,

        // ── OWS daemon mode (fallback when --rpc-url not provided) ───────────

        /// OWS wallet ID (used without --rpc-url)
        #[arg(long, default_value = "my-wallet")]
        wallet_id: String,

        /// CAIP-2 chain ID for OWS mode [default: eip155:1]
        #[arg(long, default_value = "eip155:1")]
        chain_id: String,

        /// OWS API key
        #[arg(long, default_value = "")]
        api_key: String,

        /// OWS daemon URL
        #[arg(long, default_value = ows_client::DEFAULT_OWS_URL)]
        ows_url: String,

        /// Derive and print the stealth address but don't broadcast
        #[arg(long)]
        dry_run: bool,
    },

    /// OWS policy executable — reads PolicyContext from stdin, writes PolicyResult to stdout
    Policy,

    /// Print the OWS wallet config snippet to enable shadow-wallet as a policy
    Config {
        /// OWS wallet ID to include in the snippet
        #[arg(long, default_value = "my-wallet")]
        wallet_id: String,
    },

    /// Query the live Aztec sandbox (must be running via docker compose up)
    Aztec {
        /// Aztec sandbox URL [default: http://localhost:8080]
        #[arg(long, default_value = ows_private::aztec::DEFAULT_SANDBOX_URL)]
        url: String,
    },

    /// Run a mock OWS daemon on :2512 so `send` works without installing OWS
    MockOws {
        /// Port to listen on [default: 2512]
        #[arg(long, default_value = "2512")]
        port: u16,
    },

    /// Interactive end-to-end walkthrough of the full Shadow Wallet flow
    Demo,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Cmd::Keygen { force } => cmd_keygen(force),
        Cmd::Address          => cmd_address(),
        Cmd::Derive { to }    => cmd_derive(&to),
        Cmd::Scan { announcements } => cmd_scan(announcements),
        Cmd::Send { to, value, rpc_url, private_key, announce, wallet_id, chain_id, api_key, ows_url, dry_run } => {
            cmd_send(CmdSendArgs {
                to, value, rpc_url, private_key, announce,
                wallet_id, chain_id, api_key, ows_url, dry_run,
            }).await;
        }
        Cmd::Policy               => policy_exec::run(),
        Cmd::Config { wallet_id } => cmd_config(&wallet_id),
        Cmd::Aztec { url }        => cmd_aztec(&url).await,
        Cmd::MockOws { port }     => mock_ows::run(port).await,
        Cmd::Demo                 => cmd_demo().await,
    }
}

// ---------------------------------------------------------------------------
// keygen
// ---------------------------------------------------------------------------

fn cmd_keygen(force: bool) {
    let path = keystore::identity_path();
    if path.exists() && !force {
        eprintln!("Identity already exists at {}", path.display());
        eprintln!("Use --force to overwrite.");
        std::process::exit(1);
    }

    let keys = StealthPrivateKey::generate();
    match keystore::save(&keys) {
        Ok(p) => {
            let meta = keys.public();
            println!("✓ Identity generated and saved to {}", p.display());
            println!();
            println!("Your StealthMetaAddress (share this publicly):");
            println!("  {}", meta.to_string());
            println!();
            println!("Anyone who has this address can send you private payments.");
            println!("Your viewing key lets you scan announcements — keep it local.");
        }
        Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
    }
}

// ---------------------------------------------------------------------------
// address
// ---------------------------------------------------------------------------

fn cmd_address() {
    let keys = load_or_exit();
    let meta = keys.public();
    println!("{}", meta.to_string());
}

// ---------------------------------------------------------------------------
// derive
// ---------------------------------------------------------------------------

fn cmd_derive(to_str: &str) {
    let meta = parse_meta(to_str);
    let ann = ows_private::stealth::derive_stealth(&meta)
        .unwrap_or_else(|e| { eprintln!("derivation error: {e}"); std::process::exit(1); });

    let addr = ows_private::eip55_address(&ann.stealth_address);
    println!("Stealth address : {addr}");
    println!("Ephemeral pubkey: 0x{}", hex::encode(ann.ephemeral_pubkey));
    println!("View tag        : 0x{:02x}", ann.view_tag);
    println!();
    println!("Publish this announcement on-chain via ERC5564Announcer.");
}

// ---------------------------------------------------------------------------
// scan
// ---------------------------------------------------------------------------

fn cmd_scan(file: Option<String>) {
    let keys = load_or_exit();

    let json = match file {
        Some(ref path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("Cannot read {path}: {e}"); std::process::exit(1);
        }),
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).ok();
            buf
        }
    };

    let announcements: Vec<StealthAnnouncement> = serde_json::from_str(&json)
        .unwrap_or_else(|e| { eprintln!("parse error: {e}"); std::process::exit(1); });

    let mut log = AnnouncementLog::new();
    for ann in announcements { log.push(ann); }

    let payments = log.scan(&keys)
        .unwrap_or_else(|e| { eprintln!("scan error: {e}"); std::process::exit(1); });

    if payments.is_empty() {
        println!("No incoming payments found.");
    } else {
        println!("Found {} incoming payment(s):", payments.len());
        for p in &payments {
            println!("  address : {}", p.address);
            println!("  privkey : 0x{}... (keep secret)", &hex::encode(&*p.private_key)[..16]);
            println!();
        }
    }
}

// ---------------------------------------------------------------------------
// send
// ---------------------------------------------------------------------------

struct CmdSendArgs {
    to: String, value: String,
    rpc_url: Option<String>, private_key: Option<String>, announce: bool,
    wallet_id: String, chain_id: String, api_key: String, ows_url: String,
    dry_run: bool,
}

async fn cmd_send(args: CmdSendArgs) {
    // 1. Derive stealth address for recipient
    let meta = parse_meta(&args.to);
    let ann = ows_private::stealth::derive_stealth(&meta)
        .unwrap_or_else(|e| { eprintln!("derivation error: {e}"); std::process::exit(1); });
    let stealth_addr = ows_private::eip55_address(&ann.stealth_address);

    println!("Stealth address  : {stealth_addr}");
    println!("Ephemeral pubkey : 0x{}", hex::encode(ann.ephemeral_pubkey));
    println!("View tag         : 0x{:02x}", ann.view_tag);
    println!("Value            : {} wei  ({} ETH)", args.value,
        format_eth(&args.value));
    println!();

    if args.dry_run {
        println!("Dry run — stealth address derived, nothing broadcast.");
        return;
    }

    // 2. Real EVM mode
    if let Some(ref rpc_url) = args.rpc_url {
        let key = args.private_key.as_deref().unwrap_or_else(|| {
            eprintln!("--private-key (or SHADOW_WALLET_KEY env var) required with --rpc-url");
            std::process::exit(1);
        });

        let client = evm::EvmClient::from_key(rpc_url, key)
            .unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1); });

        println!("Sender address   : {}", client.address());

        // Show balance before
        match client.get_balance(client.address()).await {
            Ok(bal) => println!("Sender balance   : {} wei  ({} ETH)\n",
                bal, format_eth(&bal.to_string())),
            Err(e) => eprintln!("  (could not fetch balance: {e})\n"),
        }

        let value_u256: U256 = args.value.parse()
            .unwrap_or_else(|_| { eprintln!("invalid value"); std::process::exit(1); });
        let to_addr: EvmAddress = stealth_addr.parse()
            .unwrap_or_else(|e| { eprintln!("bad address: {e}"); std::process::exit(1); });

        // ETH transfer
        println!("── ETH Transfer ─────────────────────────────────────────────");
        match client.send_eth(to_addr, value_u256).await {
            Ok(hash) => {
                println!("  tx hash : {hash}");
            }
            Err(e) => { eprintln!("✗ send failed: {e}"); std::process::exit(1); }
        }

        // On-chain announcement
        if args.announce {
            println!();
            println!("── ERC-5564 Announcement ────────────────────────────────────");
            match client.announce_stealth(&ann).await {
                Ok(hash) => println!("  tx hash : {hash}"),
                Err(e)   => eprintln!("✗ announce failed: {e}"),
            }
        } else {
            println!();
            println!("Tip: add --announce to publish the announcement on-chain");
            println!("so the recipient can scan for it automatically.");
        }

        println!();
        println!("✓ Done. Share this announcement with the recipient:");
        println!("  ephemeral_pubkey : 0x{}", hex::encode(ann.ephemeral_pubkey));
        println!("  stealth_address  : 0x{}", hex::encode(ann.stealth_address));
        println!("  view_tag         : 0x{:02x}", ann.view_tag);
        println!("  scheme_id        : {}", ann.scheme_id);
        return;
    }

    // 3. OWS daemon mode (fallback)
    policy_exec::register_pending(&stealth_addr)
        .unwrap_or_else(|e| eprintln!("warn: {e}"));

    let client = OwsClient::new(&args.ows_url);
    let req = SignAndSendRequest {
        chain_id:    args.chain_id.clone(),
        to:          stealth_addr.clone(),
        value:       args.value.clone(),
        data:        "0x".to_string(),
        api_key:     args.api_key.clone(),
        max_retries: Some(3),
    };

    println!("Sending via OWS ({})...", args.ows_url);
    match client.sign_and_send(&args.wallet_id, req).await {
        Ok(resp) => {
            policy_exec::clear_pending(&stealth_addr).ok();
            println!("✓ Broadcast!");
            println!("  tx hash : {}", resp.tx_hash);
            if let Some(block) = resp.block_number {
                println!("  block   : {block}");
            }
            println!("  chain   : {}", resp.chain_id);
        }
        Err(e) => { eprintln!("✗ OWS error: {e}"); std::process::exit(1); }
    }
}

fn format_eth(wei_str: &str) -> String {
    match wei_str.parse::<u128>() {
        Ok(wei) => format!("{:.6}", wei as f64 / 1e18),
        Err(_)  => "?".into(),
    }
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

fn cmd_config(wallet_id: &str) {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "shadow-wallet".to_string());

    println!("Add this to your OWS wallet config (~/.ows/wallets/{wallet_id}.json):");
    println!();
    println!(r#"{{
  "policies": [
    {{
      "id": "require-stealth",
      "description": "Block signing unless destination is a registered stealth address",
      "executable": "{exe}",
      "args": ["policy"],
      "action": "deny"
    }}
  ]
}}"#);
    println!();
    println!("Then every `ows sign` call will be evaluated by shadow-wallet first.");
    println!("Use `shadow-wallet send` to pre-register stealth addresses before signing.");
}

// ---------------------------------------------------------------------------
// aztec
// ---------------------------------------------------------------------------

async fn cmd_aztec(url: &str) {
    let bridge = AztecBridge::new(url);

    match bridge.get_node_info().await {
        Ok(info) => {
            let version = info.get("nodeVersion").and_then(|v| v.as_str()).unwrap_or("?");
            let chain   = info.get("chainId").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("Aztec sandbox  version={version}  chainId={chain}");
        }
        Err(e) => { eprintln!("Sandbox unreachable: {e}"); std::process::exit(1); }
    }

    match bridge.get_accounts().await {
        Ok(accounts) => {
            println!("{} account(s) registered:", accounts.len());
            for a in &accounts {
                println!("  {}", a.address);
            }
        }
        Err(e) => eprintln!("get_accounts: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// demo
// ---------------------------------------------------------------------------

async fn cmd_demo() {
    use std::time::Duration;

    fn sep() { println!("{}", "─".repeat(62)); }
    fn step(n: u8, title: &str) { sep(); println!("[{n}] {title}"); sep(); }
    async fn pause() { tokio::time::sleep(Duration::from_millis(600)).await; }

    println!();
    println!("  ███████╗██╗  ██╗ █████╗ ██████╗  ██████╗ ██╗    ██╗");
    println!("  ██╔════╝██║  ██║██╔══██╗██╔══██╗██╔═══██╗██║    ██║");
    println!("  ███████╗███████║███████║██║  ██║██║   ██║██║ █╗ ██║");
    println!("  ╚════██║██╔══██║██╔══██║██║  ██║██║   ██║██║███╗██║");
    println!("  ███████║██║  ██║██║  ██║██████╔╝╚██████╔╝╚███╔███╔╝");
    println!("  ╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝  ╚══╝╚══╝");
    println!();
    println!("  Shadow Wallet — OWS private agent demo");
    println!();
    pause().await;

    // ── Step 1: keygen ──────────────────────────────────────────────────────
    step(1, "Generate stealth identity  (`shadow-wallet keygen`)");
    let alice_keys = StealthPrivateKey::generate();
    keystore::save(&alice_keys).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1); });
    let alice_meta = alice_keys.public();
    println!("  ✓ Identity saved to ~/.shadow-wallet/identity.json");
    println!();
    println!("  StealthMetaAddress (share this publicly):");
    println!("  {}", alice_meta.to_string());
    pause().await;

    // ── Step 2: recipient derives their meta-address ─────────────────────────
    step(2, "Bob generates his stealth identity");
    let bob_keys = StealthPrivateKey::generate();
    let bob_meta = bob_keys.public();
    println!("  Bob's StealthMetaAddress:");
    println!("  {}", bob_meta.to_string());
    println!();
    println!("  Bob publishes this. Anyone can send to it privately.");
    pause().await;

    // ── Step 3: derive stealth address ───────────────────────────────────────
    step(3, "Derive one-time stealth address for Bob  (`shadow-wallet derive`)");
    let ann = ows_private::stealth::derive_stealth(&bob_meta)
        .unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1); });
    let stealth_addr = ows_private::eip55_address(&ann.stealth_address);
    println!("  Stealth address  : {stealth_addr}");
    println!("  Ephemeral pubkey : 0x{}", hex::encode(ann.ephemeral_pubkey));
    println!("  View tag         : 0x{:02x}", ann.view_tag);
    println!();
    println!("  This address is fresh, one-time, unlinkable to Bob's identity.");
    pause().await;

    // ── Step 4: policy rejects a plain address ────────────────────────────────
    step(4, "Policy gate — attempt send to a plain address  (`shadow-wallet policy`)");
    let plain = "0xdeadbeef000000000000000000000000deadbeef";
    let ctx_deny = serde_json::json!({
        "chainId": "eip155:1",
        "transaction": { "to": plain, "value": "500000000000000000" },
        "timestamp": "2026-04-03T00:00:00Z",
        "apiKeyId": "demo-key"
    });
    let result_deny = policy_exec::evaluate_json(&ctx_deny.to_string());
    println!("  Destination : {plain}");
    println!("  PolicyResult: {}", serde_json::to_string_pretty(&result_deny).unwrap());
    println!();
    println!("  OWS will refuse to sign. The agent cannot accidentally doxx itself.");
    pause().await;

    // ── Step 5: register + policy allows stealth address ─────────────────────
    step(5, "Policy gate — send to registered stealth address  (`shadow-wallet policy`)");
    policy_exec::register_pending(&stealth_addr).ok();
    let ctx_allow = serde_json::json!({
        "chainId": "eip155:1",
        "transaction": { "to": stealth_addr, "value": "500000000000000000" },
        "timestamp": "2026-04-03T00:00:00Z",
        "apiKeyId": "demo-key"
    });
    let result_allow = policy_exec::evaluate_json(&ctx_allow.to_string());
    println!("  Destination : {stealth_addr}");
    println!("  PolicyResult: {}", serde_json::to_string_pretty(&result_allow).unwrap());
    println!();
    println!("  OWS proceeds to sign. Key decrypted in hardened memory, wiped on exit.");
    pause().await;

    // ── Step 6: send (dry-run or mock OWS) ────────────────────────────────────
    step(6, "Sign & send via OWS  (`shadow-wallet send --dry-run`)");
    println!("  Calling OWS daemon at http://localhost:2512 ...");
    let client = OwsClient::new("http://localhost:2512");
    let req = SignAndSendRequest {
        chain_id:    "eip155:1".into(),
        to:          stealth_addr.clone(),
        value:       "500000000000000000".into(),
        data:        "0x".into(),
        api_key:     "demo-key".into(),
        max_retries: Some(3),
    };
    match client.sign_and_send("demo-wallet", req).await {
        Ok(resp) => {
            policy_exec::clear_pending(&stealth_addr).ok();
            println!("  ✓ Broadcast!");
            println!("  tx hash : {}", resp.tx_hash);
            println!("  chain   : {}", resp.chain_id);
        }
        Err(_) => {
            println!("  (OWS daemon not running — showing dry-run output)");
            println!();
            println!("  Would send 0.5 ETH to stealth address:");
            println!("  to    : {stealth_addr}");
            println!("  value : 500000000000000000 wei  (0.5 ETH)");
            println!();
            println!("  Run `shadow-wallet mock-ows` in another terminal to see the full flow.");
        }
    }
    pause().await;

    // ── Step 7: Bob scans ─────────────────────────────────────────────────────
    step(7, "Bob scans announcements  (`shadow-wallet scan`)");
    let mut log = ows_private::AnnouncementLog::new();
    log.push(ann.clone());
    let payments = log.scan(&bob_keys).unwrap_or_default();
    if payments.is_empty() {
        println!("  No payments found.");
    } else {
        println!("  Found {} incoming payment(s):", payments.len());
        for p in &payments {
            println!("  address : {}", p.address);
            println!("  privkey : 0x{}...  (zeroized on drop)", &hex::encode(&*p.private_key)[..8]);
        }
        println!();
        println!("  Bob derives the private key and can now spend from this address.");
        println!("  No on-chain observer can link it to Bob or to Alice.");
    }
    pause().await;

    // ── Step 8: Aztec ─────────────────────────────────────────────────────────
    step(8, "Aztec private L2  (`shadow-wallet aztec`)");
    let bridge = AztecBridge::sandbox();
    match bridge.get_node_info().await {
        Ok(info) => {
            let v = info.get("nodeVersion").and_then(|v| v.as_str()).unwrap_or("?");
            let c = info.get("chainId").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("  Sandbox  version={v}  chainId={c}");
            if let Ok(accounts) = bridge.get_accounts().await {
                println!("  {} Aztec account(s) registered", accounts.len());
            }
            println!();
            println!("  EVM layer  → stealth addresses hide the recipient");
            println!("  Aztec L2   → ZK proofs hide the amount + sender intent");
        }
        Err(_) => println!("  Aztec sandbox not running. Start it: docker compose up -d"),
    }

    sep();
    println!("  Shadow Wallet demo complete.");
    sep();
    println!();
}

fn load_or_exit() -> StealthPrivateKey {
    keystore::load().unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    })
}

fn parse_meta(s: &str) -> StealthMetaAddress {
    StealthMetaAddress::from_str(s).unwrap_or_else(|e| {
        eprintln!("Invalid StealthMetaAddress: {e}");
        eprintln!("Expected format: st:eth:0x<66-byte-hex-spending><66-byte-hex-viewing>");
        std::process::exit(1);
    })
}
