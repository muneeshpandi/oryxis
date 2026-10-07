use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Direction of an SSH port forward, mirroring the `ssh -L/-R/-D` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForwardKind {
    /// `-L`: a local listener tunnels to a destination reached from the server.
    Local,
    /// `-R`: a server-side listener tunnels back to a destination reached from
    /// this client.
    Remote,
    /// `-D`: a local SOCKS5 listener; the destination is chosen per-connection
    /// by the SOCKS client.
    Dynamic,
}

impl ForwardKind {
    /// All variants, in editor display order.
    pub const ALL: [ForwardKind; 3] = [
        ForwardKind::Local,
        ForwardKind::Remote,
        ForwardKind::Dynamic,
    ];

    /// Short text token used as the on-disk and on-wire representation.
    pub fn as_token(self) -> &'static str {
        match self {
            ForwardKind::Local => "local",
            ForwardKind::Remote => "remote",
            ForwardKind::Dynamic => "dynamic",
        }
    }

    /// Parse the token written by [`as_token`]. Unknown tokens fall back to
    /// `Local` so a corrupt row never breaks the whole list.
    pub fn from_token(s: &str) -> ForwardKind {
        match s {
            "remote" => ForwardKind::Remote,
            "dynamic" => ForwardKind::Dynamic,
            _ => ForwardKind::Local,
        }
    }

    /// Dynamic forwards have no fixed target (the SOCKS client picks it).
    pub fn has_target(self) -> bool {
        !matches!(self, ForwardKind::Dynamic)
    }
}

impl fmt::Display for ForwardKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ForwardKind::Local => "Local (-L)",
            ForwardKind::Remote => "Remote (-R)",
            ForwardKind::Dynamic => "Dynamic (-D)",
        };
        f.write_str(s)
    }
}

/// What the `listen_host` of a `-L` / `-D` rule asks this machine to bind.
/// The one reading of that field: the engine binds by it and the editor
/// warns by it, so the two can never disagree on what stays on this
/// machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalBind {
    /// "This machine", with no family chosen: an empty field, `localhost`,
    /// or `127.0.0.1` (the default every rule is created with). Bound on
    /// BOTH loopback families, as OpenSSH and PuTTY do: a client that
    /// writes `localhost` resolves it to `::1` first on most systems, and
    /// a listener absent from that family costs it a refused connect
    /// before it tries the other (measured at about 2 s per connection on
    /// Windows, which retries the SYN before giving up).
    Loopback,
    /// One literal address, bound exactly as written: `::1` stays IPv6
    /// only, `0.0.0.0` stays every IPv4 interface.
    Address(std::net::IpAddr),
    /// A name left to the resolver.
    Name(String),
}

impl LocalBind {
    /// Read a `listen_host`. Brackets around an IPv6 literal are accepted
    /// (`[::1]`), since that is how the address is written everywhere a
    /// port follows it.
    pub fn parse(listen_host: &str) -> Self {
        let host = listen_host.trim();
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
            return LocalBind::Loopback;
        }
        match host.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(ip)) if ip == std::net::Ipv4Addr::LOCALHOST => {
                LocalBind::Loopback
            }
            Ok(ip) => LocalBind::Address(ip),
            Err(_) => LocalBind::Name(host.to_string()),
        }
    }

    /// Whether the listener is reachable from this machine alone. A name
    /// is never assumed to be: what it resolves to is not known here.
    pub fn is_loopback(&self) -> bool {
        match self {
            LocalBind::Loopback => true,
            // `::ffff:127.0.0.1` is the loopback too, spelled as a mapped
            // IPv6 address; `is_loopback` alone reads it as a foreign v6.
            LocalBind::Address(ip) => ip.to_canonical().is_loopback(),
            LocalBind::Name(_) => false,
        }
    }
}

/// A standalone port forward, independent of any terminal session. Turning a
/// rule on opens a dedicated SSH connection (no PTY) that holds the tunnel
/// until it is turned off. Persisted in the vault; the on/off runtime state is
/// not (it lives only in app memory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardRule {
    pub id: Uuid,
    pub label: String,
    pub kind: ForwardKind,
    /// The `Connection` whose auth + transport carries this forward.
    pub host_id: Uuid,
    /// Bind interface for the listener. For `Local`/`Dynamic` this is local
    /// and read through [`LocalBind`] (`127.0.0.1` is both loopback
    /// families, `0.0.0.0` every interface); for `Remote` it is the
    /// server-side bind (`0.0.0.0` needs `GatewayPorts yes` on the remote
    /// `sshd`).
    pub listen_host: String,
    pub listen_port: u16,
    /// Destination host. `Local`: reached from the server. `Remote`: reached
    /// from this client. `Dynamic`: unused.
    pub target_host: String,
    /// Destination port. Unused for `Dynamic`.
    pub target_port: u16,
    /// Start this rule automatically on app boot.
    pub auto_start: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl PortForwardRule {
    pub fn new(label: impl Into<String>, kind: ForwardKind, host_id: Uuid) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            kind,
            host_id,
            listen_host: "127.0.0.1".to_string(),
            listen_port: 0,
            target_host: String::new(),
            target_port: 0,
            auto_start: false,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_kind_token_round_trip() {
        for kind in ForwardKind::ALL {
            assert_eq!(ForwardKind::from_token(kind.as_token()), kind);
        }
        // Unknown tokens fall back to Local rather than panicking.
        assert_eq!(ForwardKind::from_token("bogus"), ForwardKind::Local);
    }

    #[test]
    fn this_machine_is_both_loopback_families() {
        for host in ["", "  ", "localhost", "LocalHost", "127.0.0.1", " 127.0.0.1 "] {
            assert_eq!(LocalBind::parse(host), LocalBind::Loopback, "{host:?}");
        }
    }

    #[test]
    fn a_chosen_address_is_bound_as_written() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        assert_eq!(
            LocalBind::parse("::1"),
            LocalBind::Address(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
        assert_eq!(LocalBind::parse("[::1]"), LocalBind::parse("::1"));
        assert_eq!(
            LocalBind::parse("0.0.0.0"),
            LocalBind::Address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        );
        assert_eq!(
            LocalBind::parse("127.0.0.2"),
            LocalBind::Address(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)))
        );
        assert_eq!(
            LocalBind::parse("gateway.lan"),
            LocalBind::Name("gateway.lan".into())
        );
    }

    #[test]
    fn only_loopback_stays_on_this_machine() {
        for host in ["", "localhost", "127.0.0.1", "127.0.0.2", "::1", "[::1]", "::ffff:127.0.0.1"] {
            assert!(LocalBind::parse(host).is_loopback(), "{host:?}");
        }
        for host in ["0.0.0.0", "::", "192.168.1.10", "gateway.lan"] {
            assert!(!LocalBind::parse(host).is_loopback(), "{host:?}");
        }
    }

    #[test]
    fn only_dynamic_has_no_target() {
        assert!(ForwardKind::Local.has_target());
        assert!(ForwardKind::Remote.has_target());
        assert!(!ForwardKind::Dynamic.has_target());
    }
}
