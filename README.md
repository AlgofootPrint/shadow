# Shadow Wallet

A privacy layer for the [Open Wallet Standard (OWS)](https://openwallet.sh) that makes every transaction untraceable.

Built in Rust. Works on any EVM chain. Testnet-ready out of the box.

---

## What it does

Every crypto transaction is public by default — anyone can see who sent what to whom. Shadow Wallet fixes this with two layers:

**Layer 1 — Stealth Addresses (ERC-5564)**
Instead of sending to someone's real address, you derive a fresh one-time address for every transaction using ECDH. An outside observer sees a random unlinked address and has no idea who owns it. The recipient scans announcements with their private viewing key to find their funds.

**Layer 2 — Policy Engine (OWS plugin)**
A `RequireStealth` policy rule that plugs into OWS. Before OWS signs any transaction, it calls `shadow-wallet policy` as a subprocess. If the destination is not a registered stealth address, signing is blocked. The wallet literally cannot accidentally send to a traceable address.

**Layer 3 — Aztec L2**
An `AztecBridge` module that talks to an Aztec private L2 node. Combined with stealth addresses: the EVM layer hides the recipient, Aztec hides the amount.

---

## Commands

```
shadow-wallet keygen                          Generate your stealth identity
shadow-wallet address                         Print your StealthMetaAddress (share this)
shadow-wallet derive --to <meta>              Derive a one-time address for a recipient
shadow-wallet send   --to <meta> --value <wei> Send ETH to a stealth address
shadow-wallet scan   --announcements <file>   Find incoming payments addressed to you
shadow-wallet policy                          OWS policy executable (stdin/stdout)
shadow-wallet config --wallet-id <id>         Print OWS config snippet
shadow-wallet aztec                           Query the live Aztec L2 sandbox
shadow-wallet demo                            Interactive end-to-end walkthrough
shadow-wallet mock-ows                        Local signing daemon for testing
```

---

## Setup

### Prerequisites

**Rust**
```
winget install Rustlang.Rustup
```
Close your terminal, open a new one, then run:
```
rustup update stable
```
Verify: `cargo --version`

**Docker Desktop**
```
winget install Docker.DockerDesktop
```
Open Docker Desktop from the Start menu and wait for **"Engine running"** in the bottom left.

---

### 1 — Clone the repo

```
git clone https://github.com/algofootprint/shadow.git
cd shadow
```

---

### 2 — Build

```
cargo build
```

First run takes 3–5 minutes. When done:
```
Finished `dev` profile [unoptimized + debuginfo] target(s) in ...
```

---

### 3 — Start the Aztec private L2

Open a second terminal and run:
```
docker compose up -d
```

Wait 20–30 seconds, then verify:
```
docker logs openwallet-sandbox-1 --tail 3
```

You should see:
```
Aztec Sandbox v0.20.0 is now ready for use!
```

Leave this running in the background.

---

### 4 — Generate your stealth identity

```
.\target\debug\shadow-wallet.exe keygen
```

Output:
```
✓ Identity generated and saved to C:\Users\<you>\.shadow-wallet\identity.json

Your StealthMetaAddress (share this publicly):
  st:eth:0x02...
```

Your `StealthMetaAddress` is your public identity — share it with anyone who wants to send you private transactions. Your private keys stay on your machine.

---

### 5 — Run the demo

Open two terminals side by side.

**Terminal A** — start the mock signing daemon:
```
.\target\debug\shadow-wallet.exe mock-ows
```

**Terminal B** — run the walkthrough:
```
.\target\debug\shadow-wallet.exe demo
```

This steps through the full flow live: key generation, stealth derivation, policy blocking a plain address, policy allowing a stealth address, signing, broadcasting, recipient scanning, and Aztec L2 query.

---

### 6 — Try individual commands

```
rem Print your StealthMetaAddress
.\target\debug\shadow-wallet.exe address

rem Derive a one-time stealth address for a recipient
.\target\debug\shadow-wallet.exe derive --to st:eth:0x<their-meta>

rem See the OWS config snippet to register shadow-wallet as a policy plugin
.\target\debug\shadow-wallet.exe config --wallet-id my-wallet

rem Query the live Aztec sandbox
.\target\debug\shadow-wallet.exe aztec
```

---

### 7 — Send with real testnet funds (Sepolia)

No real money — Sepolia is Ethereum's public testnet.

**Get a free RPC endpoint**

Sign up at [infura.io](https://infura.io) and copy your Sepolia URL:
```
https://sepolia.infura.io/v3/YOUR_KEY
```
Or use this free endpoint (no signup):
```
https://rpc.sepolia.org
```

**Get free Sepolia test ETH**

Go to [sepoliafaucet.com](https://sepoliafaucet.com), paste your wallet address, and request funds. You'll receive 0.5 test ETH in about 30 seconds.

**Send a stealth transaction**

```
.\target\debug\shadow-wallet.exe send ^
  --to st:eth:0x<recipient-meta-address> ^
  --value 1000000000000000 ^
  --rpc-url https://rpc.sepolia.org ^
  --private-key 0x<your-wallet-private-key> ^
  --announce
```

`1000000000000000` wei = 0.001 ETH.

You'll see:
```
Stealth address  : 0x...
Ephemeral pubkey : 0x...
View tag         : 0x..

── ETH Transfer ─────────────────────────────────────────────
  Broadcasting ETH transfer...
  ✓ Confirmed in block 12345678

── ERC-5564 Announcement ────────────────────────────────────
  Publishing announcement to ERC-5564 Announcer...
  ✓ Announcement confirmed in block 12345679

✓ Done.
```

**Recipient scans for the payment**

Save the announcement details to `ann.json`:
```json
[{
  "scheme_id": 1,
  "ephemeral_pubkey": "0x...",
  "view_tag": "0x..",
  "stealth_address": "0x..."
}]
```

Then scan:
```
.\target\debug\shadow-wallet.exe scan --announcements ann.json
```

Output:
```
Found 1 incoming payment(s):
  address : 0x...
  privkey : 0x3f8a...  (keep secret)
```

Import the private key into any Ethereum wallet to spend.

---

### 8 — Plug into a real OWS wallet

Run this to get the exact config snippet:
```
.\target\debug\shadow-wallet.exe config --wallet-id my-wallet
```

Paste the output into `~/.ows/wallets/my-wallet.json`. From that point on, OWS calls `shadow-wallet policy` before every transaction automatically — no stealth address, no signature.

---

### 9 — Browser live view (optional)

**Terminal A:**
```
.\target\debug\shadow-wallet.exe mock-ows
```

**Terminal B:**
```
cargo run --example web_demo
```

Open [http://localhost:3000](http://localhost:3000).

---

## Stopping everything

```
docker compose down
```

Then `Ctrl+C` in each terminal.

---

## Troubleshooting

| Problem | Fix |
|---|---|
| `cargo` not found | Close terminal and open a new one after installing Rust |
| `docker compose` fails | Open Docker Desktop and wait for "Engine running" |
| Port already in use | `netstat -ano \| findstr :2512` then `taskkill /F /PID <pid>` |
| Aztec sandbox not ready | Wait 30s then check `docker logs openwallet-sandbox-1 --tail 3` |
| `keygen` says identity exists | Add `--force` to overwrite |
| Transaction fails — insufficient funds | Get Sepolia ETH from sepoliafaucet.com |
| `invalid private key` | Key must start with `0x` and be 64 hex characters |

---

## Project structure

```
shadow/
├── ows-private/          # Core Rust library
│   └── src/
│       ├── stealth/      # ERC-5564 key derivation, scanning, view tags
│       ├── policy/       # RequireStealth + MaxValue policy rules
│       ├── aztec/        # AztecBridge HTTP client
│       └── agent.rs      # PrivateAgent orchestrator
├── shadow-wallet/        # CLI binary
│   └── src/
│       ├── main.rs       # Commands: keygen, send, scan, policy, config, demo
│       ├── evm.rs        # Real EVM signing via alloy (Sepolia ready)
│       ├── keystore.rs   # Key persistence (~/.shadow-wallet/)
│       ├── policy_exec.rs # OWS policy executable (stdin/stdout)
│       ├── ows_client.rs # OWS daemon HTTP client
│       └── mock_ows.rs   # Local mock OWS daemon for testing
├── examples/
│   ├── shadow_demo.rs    # Terminal end-to-end demo
│   └── web_demo.rs       # Browser live view (localhost:3000)
└── docker-compose.yml    # Aztec sandbox + Anvil L1
```

---

## Built with

- [k256](https://crates.io/crates/k256) — secp256k1 ECDH and signing
- [alloy](https://crates.io/crates/alloy) — EVM transaction building and broadcasting
- [reqwest](https://crates.io/crates/reqwest) — Aztec L2 HTTP client
- [axum](https://crates.io/crates/axum) — Web demo and mock OWS server
- [clap](https://crates.io/crates/clap) — CLI

---

## Hackathon

Built for the Open Wallet Standard hackathon. Shadow Wallet is the first privacy layer for OWS — built in Rust, on the same stack OWS uses, following ERC-5564, and integrating through OWS's own policy engine.
