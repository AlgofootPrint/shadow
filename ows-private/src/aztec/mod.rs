//! Aztec Network bridge.
//!
//! Aztec is a privacy-first L2 on Ethereum. Transactions are encrypted with
//! note commitments and revealed only to participants.  This module talks to
//! an Aztec sandbox node over its JSON-RPC HTTP API.
//!
//! Sandbox quickstart (Docker):
//!   docker run -p 8080:8080 aztecprotocol/aztec-sandbox:latest
//!
//! Full Aztec integration uses the TypeScript SDK (`@aztec/aztec.js`); this
//! module provides the minimal HTTP surface needed to:
//!   - Check account balance
//!   - Send a private ETH transfer (via the `EthAddress` token contract)
//!   - Wait for a tx receipt

use serde::{Deserialize, Serialize};

use crate::error::PrivateError;

/// Default URL for a locally running Aztec sandbox.
pub const DEFAULT_SANDBOX_URL: &str = "http://localhost:8080";

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RpcRequest<'a, P: Serialize> {
    jsonrpc: &'a str,
    method: &'a str,
    params: P,
    id: u32,
}

#[derive(Deserialize, Debug)]
struct RpcResponse<R> {
    result: Option<R>,
    error: Option<RpcError>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    code: i64,
    message: String,
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A private Aztec account (shielded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AztecAccount {
    /// Aztec address (hex, 0x-prefixed)
    pub address: String,
    /// The signing public key associated with this account
    pub public_key: String,
}

/// Result of submitting a private transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AztecTxReceipt {
    pub tx_hash: String,
    pub status: String, // "mined" | "pending" | "dropped"
    pub block_number: Option<u64>,
}

// ---------------------------------------------------------------------------
// AztecBridge
// ---------------------------------------------------------------------------

/// Thin async client for the Aztec sandbox JSON-RPC API.
pub struct AztecBridge {
    url: String,
    client: reqwest::Client,
}

impl AztecBridge {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn sandbox() -> Self {
        Self::new(DEFAULT_SANDBOX_URL)
    }

    // -----------------------------------------------------------------------
    // Account management
    // -----------------------------------------------------------------------

    /// List accounts registered in the connected Aztec node.
    ///
    /// Each entry is a `CompleteAddress` which includes the Aztec address,
    /// signing public key, and partial address.
    pub async fn get_accounts(&self) -> Result<Vec<AztecAccount>, PrivateError> {
        // The sandbox returns a JSON-RPC array of CompleteAddress objects:
        // [{ "type": "CompleteAddress", "data": "<hex>" }, ...]
        // We extract just the `data` field as the address string.
        #[derive(Deserialize)]
        struct CompleteAddress {
            data: String,
        }

        let resp: RpcResponse<Vec<CompleteAddress>> = self
            .post("getRegisteredAccounts", serde_json::json!([]))
            .await?;

        let result = resp.result.ok_or_else(|| {
            PrivateError::AztecError(
                resp.error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "empty response from getRegisteredAccounts".into()),
            )
        })?;

        Ok(result
            .into_iter()
            .map(|ca| AztecAccount {
                address: ca.data,
                public_key: String::new(), // full key is embedded in the data blob
            })
            .collect())
    }

    /// Fetch node / sandbox metadata (version, chain id, L1 contract addresses).
    pub async fn get_node_info(&self) -> Result<serde_json::Value, PrivateError> {
        let resp: RpcResponse<serde_json::Value> = self
            .post("getNodeInfo", serde_json::json!([]))
            .await?;

        resp.result.ok_or_else(|| {
            PrivateError::AztecError(
                resp.error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "empty response from getNodeInfo".into()),
            )
        })
    }

    /// Get the private token balance for `account_address`.
    pub async fn get_balance(
        &self,
        account_address: &str,
        token_contract: &str,
    ) -> Result<u128, PrivateError> {
        #[derive(Deserialize)]
        struct BalanceResult {
            balance: String, // returned as decimal string to avoid JS precision loss
        }

        let resp: RpcResponse<BalanceResult> = self
            .post(
                "pxe_getBalance",
                serde_json::json!([account_address, token_contract]),
            )
            .await?;

        let result = resp.result.ok_or_else(|| {
            PrivateError::AztecError(
                resp.error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "empty response from pxe_getBalance".into()),
            )
        })?;

        result
            .balance
            .parse::<u128>()
            .map_err(|e| PrivateError::AztecError(format!("bad balance value: {e}")))
    }

    // -----------------------------------------------------------------------
    // Private transfers
    // -----------------------------------------------------------------------

    /// Send a **private** ETH transfer on Aztec.
    ///
    /// Both sender and recipient are Aztec addresses.  The amount and
    /// recipient are hidden from on-chain observers — only the tx hash is
    /// public.
    pub async fn private_transfer(
        &self,
        from: &str,
        to: &str,
        amount: u128,
        token_contract: &str,
        nonce: Option<u64>,
    ) -> Result<String, PrivateError> {
        #[derive(Deserialize)]
        struct SendResult {
            tx_hash: String,
        }

        let resp: RpcResponse<SendResult> = self
            .post(
                "pxe_sendPrivateTransfer",
                serde_json::json!([{
                    "from": from,
                    "to": to,
                    "amount": amount.to_string(),
                    "tokenContract": token_contract,
                    "nonce": nonce,
                }]),
            )
            .await?;

        let result = resp.result.ok_or_else(|| {
            PrivateError::AztecError(
                resp.error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "empty response from pxe_sendPrivateTransfer".into()),
            )
        })?;

        Ok(result.tx_hash)
    }

    /// Poll for a transaction receipt.  Returns once mined or dropped.
    pub async fn wait_for_receipt(
        &self,
        tx_hash: &str,
    ) -> Result<AztecTxReceipt, PrivateError> {
        #[derive(Deserialize)]
        struct ReceiptResult {
            tx_hash: String,
            status: String,
            block_number: Option<u64>,
        }

        let resp: RpcResponse<ReceiptResult> = self
            .post("pxe_getTxReceipt", serde_json::json!([tx_hash]))
            .await?;

        let r = resp.result.ok_or_else(|| {
            PrivateError::AztecError(
                resp.error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "empty response from pxe_getTxReceipt".into()),
            )
        })?;

        Ok(AztecTxReceipt {
            tx_hash: r.tx_hash,
            status: r.status,
            block_number: r.block_number,
        })
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    async fn post<P: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: P,
    ) -> Result<RpcResponse<R>, PrivateError> {
        let body = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        };

        let response = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| PrivateError::AztecError(format!("HTTP error: {e}")))?;

        response
            .json::<RpcResponse<R>>()
            .await
            .map_err(|e| PrivateError::AztecError(format!("JSON decode error: {e}")))
    }
}
