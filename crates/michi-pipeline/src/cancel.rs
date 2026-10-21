//! Cooperative cancellation, backed by tokio-util's `CancellationToken`.

use std::future::Future;

/// A cooperative cancellation signal, cheap to clone. Hold one clone on the side that decides to cancel and pass
/// another into [`crate::ExecOptions::cancellation`].
///
/// Wraps `tokio_util::sync::CancellationToken`, so tokio-util's 0.x type never appears in michi's public API.
/// Cancelling a token cancels every child made from it, never its parent.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(tokio_util::sync::CancellationToken);

impl CancellationToken {
    /// A token in the not-cancelled state.
    #[must_use]
    pub fn new() -> Self {
        Self(tokio_util::sync::CancellationToken::new())
    }

    /// Signals cancellation to this token and its children. Idempotent.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Whether this token, or an ancestor, has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }

    /// A child token: cancelled when this one is, and cancellable on its own without affecting this one.
    #[must_use]
    pub fn child_token(&self) -> Self {
        Self(self.0.child_token())
    }

    /// Completes once the token is cancelled; immediately if it already is.
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + '_ {
        self.0.cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_state_and_children_follow_parents_only() {
        let parent = CancellationToken::new();
        let clone = parent.clone();
        let child = parent.child_token();
        let grandchild = child.child_token();
        grandchild.cancel();
        assert!(!child.is_cancelled() && !parent.is_cancelled(), "cancelling a child never reaches its parent");
        clone.cancel();
        assert!(parent.is_cancelled() && child.is_cancelled());
    }

    #[test]
    fn cancel_is_idempotent_and_clones_share_state_both_ways() {
        let original = CancellationToken::new();
        let clone = original.clone();
        assert!(!original.is_cancelled(), "a new token starts not cancelled");
        original.cancel();
        original.cancel();
        assert!(original.is_cancelled(), "cancelling twice leaves it cancelled");
        assert!(clone.is_cancelled(), "cancelling the original reaches the clone");
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_is_awaitable() {
        let token = CancellationToken::new();
        let waiter = {
            let token = token.clone();
            tokio::spawn(async move { token.cancelled().await })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        token.cancel();
        waiter.await.unwrap();
    }

    #[test]
    fn is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CancellationToken>();
    }
}
