//! Internal subxt helpers shared between the typed pallet façades and the
//! CLI's auto-fund flow.

use subxt::{
    Config,
    client::OnlineClientAtBlockT,
    error::{TransactionProgressError, TransactionStatusError},
    transactions::{TransactionInBlock, TransactionProgress, TransactionStatus},
};

/// Drive a [`TransactionProgress`] until the transaction lands in a best
/// block (or the finalised block if subxt happens to skip straight to that
/// status), then return the resulting [`TransactionInBlock`].
///
/// Mirrors subxt's built-in `wait_for_finalized` shape but stops one rung
/// earlier — appropriate for dev/test tooling where the 12-18 s GRANDPA
/// finality lag adds nothing the SDK measures or relies on. Subxt 0.50
/// doesn't ship a `wait_for_in_block` convenience, so we drive the status
/// stream by hand: any of `Error` / `Invalid` / `Dropped` is reified into
/// the same `TransactionStatusError` subxt's own waiter would surface, so
/// callers see a uniform failure shape regardless of which waiter they use.
pub async fn wait_for_in_block<T, C>(
    mut progress: TransactionProgress<T, C>,
) -> Result<TransactionInBlock<T, C>, TransactionProgressError>
where
    T: Config,
    C: OnlineClientAtBlockT<T>,
{
    while let Some(status) = progress.next().await {
        match status? {
            // `InFinalizedBlock` is only here because `next()` can race past
            // `InBestBlock` under heavy load; treat it as the same signal.
            TransactionStatus::InBestBlock(in_block)
            | TransactionStatus::InFinalizedBlock(in_block) => return Ok(in_block),
            TransactionStatus::Error { message } => {
                return Err(TransactionStatusError::Error(message).into());
            }
            TransactionStatus::Invalid { message } => {
                return Err(TransactionStatusError::Invalid(message).into());
            }
            TransactionStatus::Dropped { message } => {
                return Err(TransactionStatusError::Dropped(message).into());
            }
            _ => continue,
        }
    }
    Err(TransactionProgressError::UnexpectedEndOfTransactionStatusStream)
}
