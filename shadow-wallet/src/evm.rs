//! Real EVM transaction signing and broadcasting via alloy.
//!
//! Used by `shadow-wallet send --rpc-url <url> --private-key <key>`.
//!
//! Supports any EVM chain — Sepolia testnet recommended for testing.
//! Sepolia RPC: https://sepolia.infura.io/v3/<YOUR_KEY>
//!              https://rpc.sepolia.org  (free, no key needed)

use alloy::{
    network::{EthereumWallet, TransactionBuilder},
    primitives::{Address, Bytes, U256, address},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    sol,
};

use ows_private::stealth::StealthAnnouncement;

// ---------------------------------------------------------------------------
// ERC-5564 Announcer contract
//
// Canonical deployment (same address on mainnet + all major testnets):
//   https://eips.ethereum.org/EIPS/eip-5564
// ---------------------------------------------------------------------------

const ANNOUNCER: Address = address!("55649E01B5Df198D18D95b5cc5051630cfD45564");

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    interface IERC5564Announcer {
        event Announcement(
            uint256 indexed schemeId,
            address indexed stealthAddress,
            address indexed caller,
            bytes ephemeralPubKey,
            bytes metadata
        );

        function announce(
            uint256 schemeId,
            address stealthAddress,
            bytes calldata ephemeralPubKey,
            bytes calldata metadata
        ) external;
    }
}

// ---------------------------------------------------------------------------
// EvmClient
// ---------------------------------------------------------------------------

pub struct EvmClient {
    rpc_url: String,
    signer:  PrivateKeySigner,
}

impl EvmClient {
    /// Create from a hex private key string (`0x...` or bare hex).
    pub fn from_key(rpc_url: &str, private_key_hex: &str) -> Result<Self, String> {
        let key = private_key_hex.trim_start_matches("0x");
        let signer: PrivateKeySigner = key.parse()
            .map_err(|e| format!("invalid private key: {e}"))?;
        Ok(Self { rpc_url: rpc_url.to_string(), signer })
    }

    /// The EVM address derived from the private key.
    pub fn address(&self) -> Address {
        self.signer.address()
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Get the ETH balance of an address in wei.
    pub async fn get_balance(&self, addr: Address) -> Result<U256, String> {
        let provider = self.provider()?;
        provider.get_balance(addr).await
            .map_err(|e| format!("eth_getBalance failed: {e}"))
    }

    /// Get the current block number.
    pub async fn block_number(&self) -> Result<u64, String> {
        let provider = self.provider()?;
        provider.get_block_number().await
            .map_err(|e| format!("eth_blockNumber failed: {e}"))
    }

    /// Get the chain ID from the RPC endpoint.
    pub async fn chain_id(&self) -> Result<u64, String> {
        let provider = self.provider()?;
        provider.get_chain_id().await
            .map_err(|e| format!("eth_chainId failed: {e}"))
    }

    // -----------------------------------------------------------------------
    // Transactions
    // -----------------------------------------------------------------------

    /// Send ETH from the signing key to `to`.
    /// Returns the confirmed tx hash.
    pub async fn send_eth(&self, to: Address, value: U256) -> Result<String, String> {
        let provider = self.wallet_provider()?;

        let tx = TransactionRequest::default()
            .with_to(to)
            .with_value(value);

        println!("  Broadcasting ETH transfer...");
        let pending = provider.send_transaction(tx).await
            .map_err(|e| format!("send_transaction failed: {e}"))?;

        let hash = *pending.tx_hash();
        println!("  Waiting for confirmation (tx: 0x{hash:x})...");

        let receipt = pending.get_receipt().await
            .map_err(|e| format!("get_receipt failed: {e}"))?;

        println!("  ✓ Confirmed in block {}", receipt.block_number.unwrap_or(0));
        Ok(format!("0x{hash:x}"))
    }

    /// Call the ERC-5564 Announcer contract to publish a stealth announcement.
    /// The sender pays the gas for this call.
    pub async fn announce_stealth(
        &self,
        ann: &StealthAnnouncement,
    ) -> Result<String, String> {
        let provider = self.wallet_provider()?;
        let announcer = IERC5564Announcer::new(ANNOUNCER, &provider);

        let stealth_addr = Address::from_slice(&ann.stealth_address);
        let ephemeral_pubkey = Bytes::copy_from_slice(&ann.ephemeral_pubkey);
        // metadata: 0x01 prefix (compressed point) per ERC-5564 spec, then view_tag
        let metadata = Bytes::from(vec![0x01, ann.view_tag]);

        println!("  Publishing announcement to ERC-5564 Announcer...");
        let pending = announcer
            .announce(
                U256::from(ann.scheme_id),
                stealth_addr,
                ephemeral_pubkey,
                metadata,
            )
            .send()
            .await
            .map_err(|e| format!("announce call failed: {e}"))?;

        let hash = *pending.tx_hash();
        println!("  Waiting for announcement confirmation (tx: 0x{hash:x})...");

        let receipt = pending.get_receipt().await
            .map_err(|e| format!("announcement receipt failed: {e}"))?;

        println!("  ✓ Announcement confirmed in block {}", receipt.block_number.unwrap_or(0));
        Ok(format!("0x{hash:x}"))
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn provider(&self) -> Result<impl Provider, String> {
        let url = self.rpc_url.parse()
            .map_err(|e| format!("invalid RPC URL: {e}"))?;
        Ok(ProviderBuilder::new().connect_http(url))
    }

    fn wallet_provider(&self) -> Result<impl Provider, String> {
        let url = self.rpc_url.parse()
            .map_err(|e| format!("invalid RPC URL: {e}"))?;
        let wallet = EthereumWallet::from(self.signer.clone());
        Ok(ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(url))
    }
}
