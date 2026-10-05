//! The SSH CONNECTION, as distinct from a session on it (F2).
//!
//! One TCP socket, one key exchange, one authentication. Channels are
//! cheap on top of it: a PTY, an SFTP subsystem and every port-forward
//! listener are all channels on the same connection, which is why a
//! second tab to a host that is already open should not pay for another
//! handshake.
//!
//! Splitting it out of `SshSession` is what makes that possible, and it
//! fixes something on the way: the RTT prober used to be per SESSION,
//! so two sessions to one host meant two pings measuring the same wire.
//! Now there is exactly one prober per connection, because the prober
//! belongs to the thing it measures.
//!
//! Lifetime is `Arc`, deliberately. Sessions, the SFTP surface and the
//! app's reuse pool all hold (or weakly hold) the same transport, and
//! the connection closes when the last owner lets go. Hand-rolled
//! reference counting is the alternative, and it is the kind that
//! leaks a live socket the first time a path forgets to decrement.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use russh::client;

use super::net_quality::{NetQuality, NetQualitySnapshot};
use super::{ClientHandler, SharedHandle};

/// An authenticated SSH connection that sessions open channels on.
pub struct SshTransport {
    handle: SharedHandle,
    /// Rolling RTT / jitter / stall figures for this CONNECTION. One
    /// prober regardless of how many sessions ride it.
    net_quality: Arc<NetQuality>,
    quality_task: tokio::task::JoinHandle<()>,
    /// Per-host `-L`/`-R`/`-D` forward tasks. They belong HERE, not to
    /// the session that happened to dial (forwards are bound once per
    /// CONNECTION; `open_session_on` binds none): owned by the dialing
    /// session they died the moment that tab closed while reused tabs
    /// kept the link alive, and no later session ever rebound them.
    /// Aborted (releasing the local listeners) when the connection
    /// itself goes.
    port_forward_tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
    /// Latched by `disconnect()` so the teardown runs exactly once even
    /// when an explicit disconnect and the `Drop` backstop both fire.
    disconnected: AtomicBool,
}

impl SshTransport {
    /// Wrap an authenticated handle and start its prober.
    pub(crate) fn new(handle: client::Handle<ClientHandler>) -> Arc<Self> {
        let handle: SharedHandle = Arc::new(handle);
        let net_quality = Arc::new(NetQuality::new());
        let quality_task = super::net_quality::spawn_quality_probe(
            Arc::clone(&handle),
            Arc::clone(&net_quality),
        );
        Arc::new(Self {
            handle,
            net_quality,
            quality_task,
            port_forward_tasks: std::sync::Mutex::new(Vec::new()),
            disconnected: AtomicBool::new(false),
        })
    }

    /// Hand the connection its per-host forward listener tasks, bound
    /// once by the dialing path. Appends, so an (unlikely) second bind
    /// on the same live connection cannot silently drop the first set.
    pub(crate) fn adopt_port_forwards(&self, tasks: Vec<tokio::task::JoinHandle<()>>) {
        match self.port_forward_tasks.lock() {
            Ok(mut held) => held.extend(tasks),
            Err(poison) => poison.into_inner().extend(tasks),
        }
    }

    /// The shared handle, for opening another channel on this
    /// connection.
    pub(crate) fn handle(&self) -> &SharedHandle {
        &self.handle
    }

    pub fn net_quality(&self) -> NetQualitySnapshot {
        self.net_quality.snapshot()
    }

    /// Whether this connection has been torn down. A pool holding a
    /// `Weak` to a transport must check this before reusing it: the
    /// `Arc` can still be alive for a moment after the disconnect.
    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::SeqCst)
    }

    /// Whether the connection looks usable for a NEW channel.
    ///
    /// The probe window is the evidence: a link whose latest ping timed
    /// out may still have a live socket, but opening a channel on it
    /// would hang until some timeout rather than fail fast. Reuse wants
    /// a cheap "probably fine", and the caller falls back to a fresh
    /// dial on any doubt, so a false negative costs one handshake while
    /// a false positive costs the user a stalled tab.
    pub fn looks_healthy(&self) -> bool {
        !self.is_disconnected() && self.net_quality().silent_for.is_none()
    }

    /// Tear the connection down: stop the prober and send the SSH
    /// disconnect. Idempotent.
    pub fn disconnect(&self) {
        if self.disconnected.swap(true, Ordering::SeqCst) {
            return;
        }
        self.quality_task.abort();
        // Release the per-host forward listeners with the connection
        // they ride on.
        {
            let held = match self.port_forward_tasks.lock() {
                Ok(g) => g,
                Err(poison) => poison.into_inner(),
            };
            for task in held.iter() {
                task.abort();
            }
        }
        // A polite disconnect needs a runtime to spawn on. Without one
        // (a late `Drop` during process shutdown) the socket dies with
        // the last handle clone anyway, which is the same outcome
        // minus the courtesy message.
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            let handle = Arc::clone(&self.handle);
            rt.spawn(async move {
                let _ = handle
                    .disconnect(russh::Disconnect::ByApplication, "session closed", "")
                    .await;
            });
        }
    }
}

impl Drop for SshTransport {
    fn drop(&mut self) {
        // The last owner letting go IS the close. Sessions no longer
        // disconnect the connection themselves precisely so that this
        // is the only place it happens.
        self.disconnect();
    }
}
