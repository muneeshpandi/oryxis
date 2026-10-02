//! Group settings inheritance (D4): what a connection's settings
//! actually resolve to once its groups have had their say.
//!
//! A host field that is unset walks up the `parent_id` chain and takes
//! the first ancestor that sets it; nothing found means the app default
//! answers, exactly as before. Resolution is PER PARAMETER, so a group
//! that only sets the proxy leaves the theme to be answered elsewhere:
//! all-or-nothing inheritance would force a user to repeat settings
//! they only wanted to share one of.
//!
//! Nothing here is stored. The chain is walked live on every read, so
//! editing a group is immediately true for every host inside it and
//! there is no copied value to go stale (the `customized_fields`
//! machinery on cloud reimport is the opposite trade, and deliberately
//! so: that one preserves what a user typed against a REMOTE source of
//! truth).

use super::*;
use oryxis_core::models::connection::{Connection, EnvVar, ProxyConfig};

/// Where a resolved value came from. The credential half of the
/// resolution is `oryxis_core::models::inheritance` (one pure ordering
/// shared with the app's listing, see that module); the proxy, the
/// theme and the snippet resolve here, over the same enum.
pub use oryxis_core::models::inheritance::Origin;

/// A connection's settings after its groups have been applied. Each
/// field carries its `Origin` so the UI can grey what is inherited and
/// offer to clear an override back to it.
#[derive(Debug, Clone, Default)]
pub struct EffectiveConfig {
    pub username: Option<(String, Origin)>,
    pub identity_id: Option<(Uuid, Origin)>,
    /// Hydrated exactly like `resolve_proxy` does it, password
    /// included.
    pub proxy: Option<(ProxyConfig, Origin)>,
    /// Merged rather than chosen: root-first, nearer scopes overriding
    /// by NAME, host last. See `merge_env`.
    pub env_vars: Vec<EnvVar>,
    pub terminal_theme: Option<(String, Origin)>,
    pub startup_snippet_id: Option<(Uuid, Origin)>,
}

impl VaultStore {
    /// The ancestry of `group_id`, nearest first: `Group::ancestry`,
    /// the one walk every reader of the chain shares, plus the log line
    /// for a chain that did not end at a root. A loop or a runaway
    /// depth is corrupt data worth a warning; a dangling parent is the
    /// ordinary mid-sync state (the ancestor's own record may simply not
    /// have arrived yet) and is not. Either way the chain walked so far
    /// is used, because a corrupt hierarchy must never be the reason a
    /// host cannot connect.
    fn ancestry<'a>(&self, groups: &'a [Group], group_id: Uuid) -> Vec<&'a Group> {
        use oryxis_core::models::group::AncestryFault;
        let walk = Group::ancestry(groups, group_id);
        match walk.fault {
            Some(AncestryFault::TooDeep(id)) => tracing::warn!(
                group = %id,
                "group ancestry hit the depth cap; resolving with what was walked so far"
            ),
            Some(AncestryFault::Loop(id)) => tracing::warn!(
                group = %id,
                "group ancestry loops; resolving with what was walked so far"
            ),
            Some(AncestryFault::Dangling(_)) | None => {}
        }
        walk.chain
    }

    /// Resolve `conn` against its group chain.
    ///
    /// `groups` and `identities` are passed in rather than listed here
    /// because the app already holds both lists, and a query per call
    /// would be a database read for nothing. The username and the
    /// identity are `effective_login`'s answer (the core ordering the
    /// listing also draws from), with the two facts only the vault can
    /// supply: whether the host stores a password of its own, and the
    /// warning for a group default that names a deleted identity.
    ///
    /// Note what is NOT resolved: the port. A group's port applies when
    /// a host is CREATED inside it, never at connect time, so a host
    /// that works today cannot change destination because a group
    /// gained a default later.
    pub fn resolve_effective(
        &self,
        conn: &Connection,
        groups: &[Group],
        identities: &[oryxis_core::models::Identity],
    ) -> Result<EffectiveConfig, VaultError> {
        let chain: Vec<&Group> = match conn.group_id {
            Some(gid) => self.ancestry(groups, gid),
            None => Vec::new(),
        };

        // Credentials are ONE parameter family: a host that stores its
        // own password or names its own key has answered it, so a group
        // identity default must not eclipse what it can already do. The
        // key is on the row; the password is a column only the vault can
        // ask, and `NotFound` (a host not yet saved) reads as "none".
        let host_answers_credentials =
            conn.key_id.is_some() || self.connection_has_password(&conn.id).unwrap_or(false);
        for group in &chain {
            if let Some(id) = group.defaults.as_ref().and_then(|d| d.identity_id)
                && !identities.iter().any(|i| i.id == id)
            {
                // The resolver skips it (a dangling reference would take
                // the credential branch and resolve to no credentials at
                // all); the warning is what tells the user why.
                tracing::warn!(
                    identity = %id,
                    group = %group.id,
                    "group default identity not found, leaving the host on its own credentials"
                );
            }
        }
        let login = oryxis_core::models::inheritance::effective_login(
            conn,
            &chain,
            identities,
            host_answers_credentials,
        );

        let mut effective = EffectiveConfig {
            username: login.username,
            identity_id: login.identity_id,
            terminal_theme: conn
                .terminal_theme
                .clone()
                .filter(|t| !t.is_empty())
                .map(|t| (t, Origin::Host)),
            startup_snippet_id: conn.startup_snippet_id.map(|id| (id, Origin::Host)),
            // The host's own proxy keeps every rule `resolve_proxy`
            // already documents (identity over inline, dangling id ->
            // None with a warning). Layering rather than replacing is
            // what keeps those callers correct.
            proxy: self.resolve_proxy(conn)?.map(|p| (p, Origin::Host)),
            env_vars: Vec::new(),
        };

        for group in &chain {
            let Some(defaults) = group.defaults.as_ref() else {
                continue;
            };
            let origin = Origin::Group(group.id);
            if effective.terminal_theme.is_none()
                && let Some(t) = defaults.terminal_theme.clone().filter(|t| !t.is_empty())
            {
                effective.terminal_theme = Some((t, origin));
            }
            if effective.startup_snippet_id.is_none()
                && let Some(id) = defaults.startup_snippet_id
            {
                effective.startup_snippet_id = Some((id, origin));
            }
            if effective.proxy.is_none()
                && let Some(pid) = defaults.proxy_identity_id
            {
                // Same forgiveness as `resolve_proxy`: an identity the
                // user deleted leaves the host proxy-less with a
                // warning rather than breaking every host under the
                // group that referenced it.
                match self.get_proxy_identity(&pid)? {
                    Some(ident) => {
                        let password = self.get_proxy_identity_password(&pid).ok().flatten();
                        effective.proxy = Some((
                            ProxyConfig {
                                proxy_type: ident.proxy_type,
                                host: ident.host,
                                port: ident.port,
                                username: ident.username,
                                password,
                            },
                            origin,
                        ));
                    }
                    None => tracing::warn!(
                        proxy_identity = %pid,
                        group = %group.id,
                        "group default proxy identity not found, leaving the host without a proxy"
                    ),
                }
            }
        }

        effective.env_vars = merge_env(&chain, &conn.env_vars);
        Ok(effective)
    }

    /// Collapse [`resolve_effective`](Self::resolve_effective) onto the
    /// working copy an engine dials: the effective proxy lands on
    /// `conn.proxy` (the only proxy field engines read), an inherited
    /// username / identity fills the empty fields, and the merged env
    /// set replaces the host's own.
    ///
    /// The SSH engine reads `connection.username` and falls back to
    /// `DEFAULT_USERNAME`, it never looks inside an identity, so a host
    /// that names no user and resolves to an identity takes the
    /// identity's username here (`effective_login` resolves it, which is
    /// also what the listing shows).
    ///
    /// Resolution failing must not stop a connect that would otherwise
    /// work: the fallback is the host's own proxy, exactly the pre-D4
    /// behaviour. ONE implementation for every consumer (the app's
    /// dial sites and the MCP server), so the two can't drift.
    pub fn apply_effective(
        &self,
        conn: &mut Connection,
        groups: &[Group],
        identities: &[oryxis_core::models::Identity],
    ) {
        let effective = match self.resolve_effective(conn, groups, identities) {
            Ok(effective) => effective,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "group inheritance failed, using the host's own settings"
                );
                conn.proxy = self.resolve_proxy(conn).ok().flatten();
                return;
            }
        };
        conn.proxy = effective.proxy.map(|(p, _)| p);
        if conn.username.as_deref().unwrap_or_default().is_empty() {
            conn.username = effective.username.map(|(u, _)| u);
        }
        if conn.identity_id.is_none() {
            conn.identity_id = effective.identity_id.map(|(id, _)| id);
        }
        // Already merged by name with the host winning, so this is the
        // whole set rather than an override.
        conn.env_vars = effective.env_vars;
    }
}

/// Environment variables are MERGED, not chosen.
///
/// `Vec<EnvVar>` has no "unset" to distinguish from "empty", so the
/// override rule other fields use cannot apply: a host with no
/// variables would otherwise mean "override the group with nothing",
/// and the group's variables would vanish the moment a host defined
/// one of its own. Merging by name keeps both, and a host that names
/// the same variable wins, which is the only reading in which a host
/// can still say no to an inherited value.
///
/// `chain` is nearest-first, so it is applied in REVERSE: the farthest
/// ancestor lays the base and nearer scopes overwrite it, host last.
fn merge_env(chain: &[&Group], host: &[EnvVar]) -> Vec<EnvVar> {
    let mut merged: Vec<EnvVar> = Vec::new();
    let mut put = |var: &EnvVar| {
        if let Some(existing) = merged.iter_mut().find(|e| e.key == var.key) {
            existing.value = var.value.clone();
        } else {
            merged.push(var.clone());
        }
    };
    for group in chain.iter().rev() {
        if let Some(defaults) = group.defaults.as_ref() {
            for var in &defaults.env_vars {
                put(var);
            }
        }
    }
    for var in host {
        put(var);
    }
    merged
}
