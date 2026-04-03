//! Thin async client for the OWS daemon REST API.
//!
//! OWS daemon listens on http://localhost:2512 by default.
//! Docs: https://docs.openwallet.sh/api
//!
//! Endpoints used:
//!   POST /wallets/:wallet_id/sign-and-send
//!   GET  /wallets/:wallet_id

use serde::{Deserialize, Serialize};

pub const DEFAULT_OWS_URL: &str = "http://localhost:2512";

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Outgoing sign-and-send request body.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignAndSendRequest {
    pub chain_id:    String,  // CAIP-2, e.g. "eip155:1" or "eip155:31337"
    pub to:          String,  // 0x-prefixed EVM address
    pub value:       String,  // wei as decimal string
    pub data:        String,  // 0x-prefixed calldata (empty = "0x")
    pub api_key:     String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

/// Successful response from sign-and-send.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SignAndSendResponse {
    pub tx_hash:      String,
    pub block_number: Option<u64>,
    pub chain_id:     String,
}

/// Error body returned by OWS on 4xx/5xx.
#[derive(Deserialize, Debug)]
pub struct OwsError {
    pub error:   String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct OwsClient {
    base_url: String,
    http:     reqwest::Client,
}

impl OwsClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), http: reqwest::Client::new() }
    }

    pub fn local() -> Self { Self::new(DEFAULT_OWS_URL) }

    /// Sign a transaction and broadcast it via the OWS daemon.
    pub async fn sign_and_send(
        &self,
        wallet_id: &str,
        req: SignAndSendRequest,
    ) -> Result<SignAndSendResponse, String> {
        let url = format!("{}/wallets/{}/sign-and-send", self.base_url, wallet_id);

        let resp = self.http
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("OWS unreachable: {e}\n  Is the OWS daemon running? (`ows start`)"))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            serde_json::from_str::<SignAndSendResponse>(&body)
                .map_err(|e| format!("parse OWS response: {e}\n  body: {body}"))
        } else {
            // Try to extract a structured error message
            let msg = serde_json::from_str::<OWSErrorBody>(&body)
                .map(|e| format!("{}: {}", e.error, e.message))
                .unwrap_or_else(|_| body);
            Err(format!("OWS returned {status}: {msg}"))
        }
    }
}

#[derive(Deserialize)]
struct OWSErrorBody {
    error: String,
    message: String,
}
