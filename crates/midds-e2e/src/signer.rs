//! Dev signers.
//!
//! Every Substrate `--dev` chain pre-funds the canonical dev accounts and
//! gives sudo to `//Alice`. The e2e suite signs every extrinsic with one of
//! these — tests never touch on-chain key generation.

use subxt_signer::sr25519::{Keypair, dev};

/// Sudo + pre-funded dev account. Used both as a regular depositor and as
/// the `ForceOrigin` for sudo-wrapped admin calls.
#[must_use]
pub fn alice() -> Keypair {
    dev::alice()
}

/// Pre-funded secondary dev account — useful when a test needs a second
/// independent signer (e.g. to prove an extrinsic refuses the wrong caller).
#[must_use]
pub fn bob() -> Keypair {
    dev::bob()
}
