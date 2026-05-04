//! Single source of truth for the metadata keys (storage names, constant
//! names, event names, extrinsic names) the dynamic subxt path resolves
//! against the runtime metadata.
//!
//! Centralised here so a runtime-side rename (`Items` → `Records`,
//! `Deposited` → `Stored`) is a one-line change instead of a
//! grep-the-codebase exercise. Pallet metadata names match `pallet-midds`'s
//! Rust identifiers exactly — they're emitted by `#[pallet::storage]` and
//! `#[pallet::event]` macros — so any drift here is a runtime SDK
//! mismatch, not an upstream Polkadot SDK ambiguity.

// ---- pallet-midds extrinsics ----------------------------------------------

/// `pallet-midds::deposit` extrinsic name.
pub(crate) const DEPOSIT_CALL: &str = "deposit";

// ---- pallet-midds storage --------------------------------------------------

/// Per-instance monotonic id counter.
pub(crate) const NEXT_MIDDS_ID_STORAGE: &str = "NextMiddsId";

// ---- pallet-midds constants -----------------------------------------------

/// Flat part of the bond formula.
pub(crate) const DEPOSIT_BASE_CONST: &str = "DepositBase";
/// Per-byte multiplier of the bond formula.
pub(crate) const DEPOSIT_PER_BYTE_CONST: &str = "DepositPerByte";

// ---- pallet-midds events --------------------------------------------------

/// Successful deposit event — emitted once per inner extrinsic, including
/// inside a `pallet_utility::batch_all` outer call.
pub(crate) const DEPOSITED_EVENT: &str = "Deposited";

// ---- pallet_transaction_payment events -------------------------------------

/// Pallet name carrying the inclusion-fee event.
pub(crate) const TX_PAYMENT_PALLET: &str = "TransactionPayment";
/// Inclusion-fee event name.
pub(crate) const TX_FEE_PAID_EVENT: &str = "TransactionFeePaid";
