//! Minimal mock OWS daemon for local testing.
//!
//! Listens on http://localhost:2512 and implements just enough of the
//! OWS REST API to let `shadow-wallet send` work without a real OWS install.
//!
//! Endpoints:
//!   POST /wallets/:wallet_id/sign-and-send  → fake tx hash
//!   GET  /status                            → {"status":"ok"}

use axum::{
    Router,
    extract::{Path, Json},
    response::Json as ResJson,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignRequest {
    chain_id:    Option<String>,
    to:          Option<String>,
    value:       Option<String>,
    #[serde(default)]
    data:        String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignResponse {
    tx_hash:      String,
    block_number: u64,
    chain_id:     String,
    status:       String,
}

async fn sign_and_send(
    Path(wallet_id): Path<String>,
    Json(req): Json<SignRequest>,
) -> ResJson<Value> {
    let to    = req.to.as_deref().unwrap_or("0x0");
    let value = req.value.as_deref().unwrap_or("0");
    let chain = req.chain_id.unwrap_or_else(|| "eip155:1".into());

    // Deterministic fake tx hash from inputs
    let hash_input = format!("{wallet_id}{to}{value}");
    let hash_bytes = sha256_bytes(hash_input.as_bytes());
    let tx_hash = format!("0x{}", hex::encode(hash_bytes));

    println!(
        "  [mock-ows] sign-and-send  wallet={wallet_id}  to={to}  value={value} wei  chain={chain}"
    );
    println!("  [mock-ows] → tx_hash={tx_hash}");

    ResJson(json!(SignResponse {
        tx_hash,
        block_number: 42,
        chain_id: chain,
        status: "mined".into(),
    }))
}

async fn status() -> ResJson<Value> {
    ResJson(json!({ "status": "ok", "version": "mock-0.1.0", "note": "shadow-wallet mock OWS" }))
}

/// Run the mock OWS daemon. Blocks until Ctrl-C.
pub async fn run(port: u16) {
    let app = Router::new()
        .route("/wallets/:wallet_id/sign-and-send", post(sign_and_send))
        .route("/status", get(status));

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await
        .unwrap_or_else(|e| {
            eprintln!("Cannot bind to {addr}: {e}");
            std::process::exit(1);
        });

    println!("Mock OWS daemon listening on http://localhost:{port}");
    println!("  POST /wallets/:id/sign-and-send  — accepts sign requests");
    println!("  GET  /status                     — health check");
    println!();
    println!("Now run in another terminal:");
    println!("  shadow-wallet send --to <meta> --value 500000000000000000 --wallet-id demo-wallet");
    println!();
    println!("Press Ctrl-C to stop.");

    axum::serve(listener, app).await.unwrap();
}

// ---------------------------------------------------------------------------
// tiny SHA-256 helper (no extra dep — k256 already brings sha2)
// ---------------------------------------------------------------------------

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}
