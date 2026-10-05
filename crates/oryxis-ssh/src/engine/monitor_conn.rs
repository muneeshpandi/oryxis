//! Headless, probe-only connection for the multi-host monitor
//! dashboard (issue #95).
//!
//! A [`MonitorConn`] is the authenticated transport and nothing else:
//! no PTY, no shell channel, no reader/writer tasks. The host sees a
//! login and one short-lived exec channel per poll, exactly like the
//! per-session monitor's side channels, so an idle dashboard entry
//! costs the server nothing but a TCP connection. Hosts with a live
//! terminal tab never go through here (the dashboard reuses the tab's
//! [`SshSession`](super::SshSession) instead).

use super::*;

/// A probe-only SSH connection: the shared authenticated handle plus a
/// close latch. Cheap to clone via `Arc` at the call sites.
pub struct MonitorConn {
    handle: SharedHandle,
    /// Latched by [`close`](Self::close) so teardown runs exactly once
    /// even when both an explicit close and the `Drop` backstop fire.
    closed: std::sync::atomic::AtomicBool,
}

impl MonitorConn {
    pub(crate) fn new(handle: client::Handle<ClientHandler>) -> Self {
        Self {
            handle: Arc::new(handle),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Run a short, silent command and return its stdout. Same contract
    /// (and same implementation) as [`SshSession::probe`]: bounded
    /// output, bounded time, nothing else on the connection blocked.
    pub async fn probe(
        &self,
        command: &str,
        timeout: std::time::Duration,
    ) -> Option<String> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        super::session::probe_on(&self.handle, command, timeout).await
    }

    /// Run a command on this connection and return its full result.
    /// The body is [`exec_capture_on`](super::session::exec_capture_on);
    /// see it for the two bounds and the cancel contract. A closed
    /// connection answers with a channel error, the same reading a
    /// dead link gets, so a caller pooling these treats both alike.
    pub async fn exec_capture(
        &self,
        command: &str,
        stdin: Option<Vec<u8>>,
        open_timeout: std::time::Duration,
        run_timeout: std::time::Duration,
        cancel: Option<tokio::sync::watch::Receiver<bool>>,
    ) -> Result<ExecResult, SshError> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(SshError::Channel("connection closed".into()));
        }
        super::session::exec_capture_on(
            &self.handle,
            command,
            stdin,
            open_timeout,
            run_timeout,
            cancel,
        )
        .await
    }

    /// Whether the transport can still carry a probe. Without reader /
    /// writer tasks the only signals are the close latch and the russh
    /// handle's own closed flag (set when the peer drops the TCP
    /// connection or sends a disconnect).
    pub fn is_alive(&self) -> bool {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        !self.handle.is_closed()
    }

    /// Tear the connection down and wait for the disconnect to go out.
    /// The awaited twin of [`close`](Self::close), for a shutdown that
    /// wants the courtesy message on the wire before the process ends;
    /// a spawned task would be cut off by `process::exit`. Idempotent
    /// with `close`: whichever latches first does the work.
    pub async fn disconnect(&self) {
        use std::sync::atomic::Ordering;
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "monitor closed", "")
            .await;
    }

    /// Tear the connection down. Idempotent: only the first call acts.
    pub fn close(&self) {
        use std::sync::atomic::Ordering;
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // Politely disconnect the transport; outside a runtime (late
        // Drop during shutdown) the TCP socket dies with the handle.
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            let handle = Arc::clone(&self.handle);
            rt.spawn(async move {
                let _ = handle
                    .disconnect(russh::Disconnect::ByApplication, "monitor closed", "")
                    .await;
            });
        }
    }
}

impl Drop for MonitorConn {
    fn drop(&mut self) {
        self.close();
    }
}

impl std::fmt::Debug for MonitorConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitorConn")
            .field(
                "closed",
                &self.closed.load(std::sync::atomic::Ordering::SeqCst),
            )
            .finish()
    }
}
