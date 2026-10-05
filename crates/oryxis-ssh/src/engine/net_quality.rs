use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Rolling transport-quality figures for one SSH session, fed by the
/// per-session prober task (see `spawn_quality_probe`) and read by the
/// terminal performance HUD (and, later, the per-tab latency indicator).
///
/// Latency is the round trip of a `keepalive@openssh.com` global request
/// (russh's `send_ping`): every server replies (REQUEST_SUCCESS or
/// REQUEST_FAILURE, either proves liveness), so the timing is valid
/// regardless of the verdict.
///
/// Raw packet loss is deliberately NOT reported: SSH rides TCP, where the
/// kernel retransmits silently, so loss is invisible at this layer. It
/// manifests as RTT spikes and stalls instead, which is exactly what
/// `peak_rtt`, `jitter`, `timeouts` and `silent_for` surface.
pub struct NetQuality {
    inner: std::sync::Mutex<NetQualityInner>,
}

struct NetQualityInner {
    /// Last `PROBE_WINDOW` probe outcomes, oldest first: `Some(rtt)` for
    /// a reply within `PROBE_TIMEOUT`, `None` for a timeout.
    probes: VecDeque<Option<Duration>>,
    /// When the server last answered a probe.
    last_reply_at: Option<Instant>,
    /// When probing began, the fallback anchor for `silent_for` on a
    /// session whose server never answered a single probe.
    started_at: Instant,
}

/// One probe every 3 s: cheap enough to run for the session's lifetime
/// (a single tiny transport packet), frequent enough that the HUD's
/// window reflects the last minute of link behavior.
pub(crate) const PROBE_INTERVAL: Duration = Duration::from_secs(3);
/// A probe unanswered for this long counts as a timeout. Interactive
/// SSH is unusable well before a 2 s round trip, so nothing meaningful
/// is lost by giving up and probing again.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Probe outcomes retained, ~1 minute of history at `PROBE_INTERVAL`.
const PROBE_WINDOW: usize = 20;

/// Spawn the per-session RTT prober. It pings forever at
/// `PROBE_INTERVAL`; it exits on its own when the session's transport
/// is gone (the ping send errors) and is aborted by
/// `SshSession::close` as a backstop.
pub(crate) fn spawn_quality_probe(
    handle: super::SharedHandle,
    quality: std::sync::Arc<NetQuality>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(PROBE_INTERVAL).await;
            // The shared handle has no lock, so the ping queues behind
            // nothing on this side: what it times is the round trip.
            let outcome = {
                let start = Instant::now();
                match tokio::time::timeout(PROBE_TIMEOUT, handle.send_ping()).await {
                    Ok(Ok(())) => Some(start.elapsed()),
                    // Transport torn down; stop probing.
                    Ok(Err(_)) => return,
                    Err(_) => None,
                }
            };
            match outcome {
                Some(rtt) => quality.record_rtt(rtt),
                None => quality.record_timeout(),
            }
        }
    })
}

/// Point-in-time copy of the rolling figures, safe to hand to the UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct NetQualitySnapshot {
    /// Most recent successful round trip.
    pub last_rtt: Option<Duration>,
    /// Mean round trip over the window's successful probes.
    pub avg_rtt: Option<Duration>,
    /// Worst round trip in the window; TCP retransmissions after real
    /// packet loss show up here as spikes.
    pub peak_rtt: Option<Duration>,
    /// Mean absolute difference between consecutive round trips
    /// (RFC 3550-style interarrival jitter over the probe stream).
    pub jitter: Option<Duration>,
    /// Probes in the window that hit `PROBE_TIMEOUT` without a reply.
    pub timeouts: usize,
    /// How long the server has been silent, reported only while the
    /// latest probe outcome is a timeout (the mosh-style "no reply for
    /// Ns" signal). `None` while the link answers.
    pub silent_for: Option<Duration>,
}

impl NetQuality {
    pub(crate) fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(NetQualityInner {
                probes: VecDeque::with_capacity(PROBE_WINDOW),
                last_reply_at: None,
                started_at: Instant::now(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, NetQualityInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn record_rtt(&self, rtt: Duration) {
        let mut inner = self.lock();
        inner.last_reply_at = Some(Instant::now());
        push_probe(&mut inner.probes, Some(rtt));
    }

    pub(crate) fn record_timeout(&self) {
        push_probe(&mut self.lock().probes, None);
    }

    pub fn snapshot(&self) -> NetQualitySnapshot {
        self.lock().snapshot_at(Instant::now())
    }
}

fn push_probe(probes: &mut VecDeque<Option<Duration>>, outcome: Option<Duration>) {
    probes.push_back(outcome);
    while probes.len() > PROBE_WINDOW {
        probes.pop_front();
    }
}

impl NetQualityInner {
    fn snapshot_at(&self, now: Instant) -> NetQualitySnapshot {
        let ok: Vec<Duration> = self.probes.iter().filter_map(|p| *p).collect();
        let avg_rtt = (!ok.is_empty())
            .then(|| ok.iter().sum::<Duration>() / ok.len() as u32);
        let jitter = (ok.len() >= 2).then(|| {
            let diffs: Duration = ok
                .windows(2)
                .map(|w| w[1].abs_diff(w[0]))
                .sum();
            diffs / (ok.len() - 1) as u32
        });
        let silent_for = matches!(self.probes.back(), Some(None)).then(|| {
            now.saturating_duration_since(self.last_reply_at.unwrap_or(self.started_at))
        });
        NetQualitySnapshot {
            last_rtt: ok.last().copied(),
            avg_rtt,
            peak_rtt: ok.iter().max().copied(),
            jitter,
            timeouts: self.probes.iter().filter(|p| p.is_none()).count(),
            silent_for,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn snapshot_aggregates_rtt_window() {
        let q = NetQuality::new();
        for rtt in [20, 30, 100, 30] {
            q.record_rtt(ms(rtt));
        }
        let s = q.snapshot();
        assert_eq!(s.last_rtt, Some(ms(30)));
        assert_eq!(s.avg_rtt, Some(ms(45)));
        assert_eq!(s.peak_rtt, Some(ms(100)));
        // |30-20| + |100-30| + |30-100| = 150 over 3 transitions.
        assert_eq!(s.jitter, Some(ms(50)));
        assert_eq!(s.timeouts, 0);
        assert!(s.silent_for.is_none());
    }

    #[test]
    fn timeouts_count_and_latest_timeout_reports_silence() {
        let q = NetQuality::new();
        q.record_rtt(ms(25));
        q.record_timeout();
        q.record_timeout();
        let s = q.snapshot();
        assert_eq!(s.timeouts, 2);
        // RTT figures still reflect the last success.
        assert_eq!(s.last_rtt, Some(ms(25)));
        assert!(s.silent_for.is_some());

        // A reply ends the silence.
        q.record_rtt(ms(30));
        assert!(q.snapshot().silent_for.is_none());
    }

    #[test]
    fn silence_on_a_never_answering_server_uses_probe_start() {
        let q = NetQuality::new();
        q.record_timeout();
        let s = q.snapshot();
        assert!(s.silent_for.is_some());
        assert!(s.last_rtt.is_none());
        assert!(s.avg_rtt.is_none());
        assert!(s.jitter.is_none());
    }

    #[test]
    fn window_drops_oldest_probes() {
        let q = NetQuality::new();
        for _ in 0..PROBE_WINDOW {
            q.record_timeout();
        }
        for _ in 0..PROBE_WINDOW {
            q.record_rtt(ms(10));
        }
        let s = q.snapshot();
        assert_eq!(s.timeouts, 0);
        assert_eq!(s.avg_rtt, Some(ms(10)));
    }
}
