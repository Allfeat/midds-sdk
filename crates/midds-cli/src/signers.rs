//! Signer parsing and deterministic derivation helpers shared by CLI commands.
//!
//! Multi-account workloads (`seed`, `bench`) need many signers reproducible
//! across runs. Substrate's secret URI scheme already supports arbitrary
//! derivation paths, so we just append `//<n>` to a base URI and let
//! `subxt-signer` do the work.

use std::str::FromStr;

use anyhow::{Result, anyhow};
use midds_client::subxt_signer::{SecretUri, sr25519::Keypair};

/// Parse a single signer URI into a sr25519 [`Keypair`].
///
/// Accepts everything `subxt-signer` does: `//Alice`, BIP-39 mnemonics, raw
/// `0x<hex-seed>`, and any combination with hard or soft junctions.
pub fn build_signer(uri: &str) -> Result<Keypair> {
    let secret = SecretUri::from_str(uri).map_err(|e| anyhow!("parse signer URI `{uri}`: {e}"))?;
    Keypair::from_uri(&secret).map_err(|e| anyhow!("derive keypair from `{uri}`: {e}"))
}

/// Derive `count` distinct signers under `base_uri` via the standard `//<n>`
/// hard-junction scheme, with `n` ranging over `1..=count`.
///
/// Reproducibility contract: same `(base_uri, count)` always yields the same
/// list of `Keypair`s — backed entirely by sr25519 derivation, no RNG involved.
/// Identical signers can therefore be re-derived by `seed` and `bench` runs
/// on the same operator machine.
pub fn derive_signers(base_uri: &str, count: u32) -> Result<Vec<Keypair>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    (1..=count)
        .map(|i| {
            let uri = format!("{base_uri}//{i}");
            build_signer(&uri)
        })
        .collect()
}
