//! Error types surfaced by [`midds-client`](crate).

use midds_traits::MiddsFormatError;
use subxt::error as subxt_err;

/// Errors returned by the high-level [`MiddsClient`](crate::MiddsClient) API.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// MIDDS payload failed format validation before submission.
    #[error("invalid MIDDS format: {0:?}")]
    InvalidFormat(MiddsFormatError),

    /// Underlying subxt error (connection, signing, submission, decode, …).
    #[error(transparent)]
    Subxt(#[from] subxt::Error),

    /// SCALE decoding of a runtime response or event payload failed.
    #[error("scale decode: {0}")]
    Decode(#[from] parity_scale_codec::Error),

    /// Expected event was not emitted in the finalized block.
    #[error("expected `{pallet}::{variant}` event was not emitted")]
    EventNotFound {
        /// Pallet name we searched in.
        pallet: &'static str,
        /// Event variant name we searched for.
        variant: &'static str,
    },

    /// `chain_getBlockHash(None)` returned `null` — the node has no best
    /// block yet. Surfaces as a hard error rather than a retry signal:
    /// any node we're meant to talk to is past genesis.
    #[error("node has no best block yet (chain_getBlockHash returned null)")]
    NoBestBlock,
}

impl From<MiddsFormatError> for Error {
    fn from(value: MiddsFormatError) -> Self {
        Self::InvalidFormat(value)
    }
}

/// Subxt's typed sub-error kinds wrap into `subxt::Error` directly, but `?`
/// only triggers that conversion when the error position is shaped as
/// `subxt::Error`. Several call sites in this crate `?` a sub-kind directly
/// (e.g. `at_current_block()` returns `OnlineClientAtBlockError`), so we
/// route those through `subxt::Error` explicitly.
macro_rules! from_subxt_subkind {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for Error {
                fn from(value: $ty) -> Self {
                    Self::Subxt(value.into())
                }
            }
        )*
    };
}

from_subxt_subkind! {
    subxt_err::OnlineClientError,
    subxt_err::OnlineClientAtBlockError,
    subxt_err::ExtrinsicError,
    subxt_err::TransactionProgressError,
    subxt_err::TransactionEventsError,
    subxt_err::EventsError,
    subxt_err::StorageError,
    subxt_err::ConstantError,
    subxt_err::RuntimeApiError,
    // Raised by `chain_getBlockHash` and other raw RPC calls in
    // `MiddsClient::at_best_block`; routed through `Subxt` via
    // `subxt::Error::OtherRpcClientError` so callers keep one error arm.
    subxt::rpcs::Error,
}
