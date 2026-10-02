//! The login a dial will use once a host's groups have had their say
//! (D4), resolved by ONE pure function.
//!
//! The SSH engine reads `Connection.username` and falls back to
//! [`DEFAULT_USERNAME`]; it never looks inside an identity or a group.
//! Everything that fills that field before the dial, and everything
//! that SAYS what the field will be (the dashboard card, the hosts
//! tree, the tab's second line, the copied `ssh://` URL), has to agree,
//! and two implementations of the same ordering drifted twice already:
//! a host inheriting its user once dialled as root on the path that
//! forgot the group, and once showed root on the card while dialling as
//! the group said. So the ordering lives here, with no database and no
//! app state behind it, and the vault's resolver and the app's views
//! both call it with what they hold.
//!
//! The order, which is not obvious: the host's own username, else the
//! nearest group that sets one (at ANY depth, so a farther group's
//! username beats a nearer group's identity), else the username of the
//! identity the host resolves to, where that identity is the host's own
//! or, for a host that answers no credential of its own, the nearest
//! group's. An empty string counts as unset everywhere.

use uuid::Uuid;

use super::connection::Connection;
use super::group::Group;
use super::identity::Identity;

/// What the engine logs in as when nothing names a user
/// (`oryxis-ssh`'s `effective_username`). The listing shows the same
/// word for the same reason: it is what will happen.
pub const DEFAULT_USERNAME: &str = "root";

/// Where a resolved value came from, so the editor can say "inherited
/// from <group>" instead of pretending the host set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The host names it itself.
    Host,
    /// A group in the host's ancestry, nearest first.
    Group(Uuid),
}

/// The credential half of a host's effective configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectiveLogin {
    pub username: Option<(String, Origin)>,
    pub identity_id: Option<(Uuid, Origin)>,
}

impl EffectiveLogin {
    /// The resolved username, or the engine's own fallback.
    pub fn username_or_default(&self) -> &str {
        self.username
            .as_ref()
            .map(|(u, _)| u.as_str())
            .unwrap_or(DEFAULT_USERNAME)
    }
}

/// Resolve the login of `conn` against `chain`, its ancestry nearest
/// first (`Group::ancestry`), and `identities`, the full list.
///
/// `host_answers_credentials` is whether the host stores a password or
/// names a key of its own. Credentials are ONE parameter family: the
/// engine's credential resolution takes the identity branch wholesale,
/// so inheriting a group identity onto such a host would silently
/// disable what it has the day the group gains a default. The caller
/// answers it because only the caller knows whether a password is
/// stored (the vault asks its column, the app asks its cached set); the
/// host's OWN identity is never gated, naming one IS answering.
///
/// A group default naming an identity that is not in `identities` is
/// skipped: a dangling reference would also take the credential branch
/// and resolve to no credentials at all.
pub fn effective_login(
    conn: &Connection,
    chain: &[&Group],
    identities: &[Identity],
    host_answers_credentials: bool,
) -> EffectiveLogin {
    let non_empty = |s: Option<&str>| s.map(str::to_string).filter(|s| !s.is_empty());
    let mut login = EffectiveLogin {
        username: non_empty(conn.username.as_deref()).map(|u| (u, Origin::Host)),
        identity_id: conn.identity_id.map(|id| (id, Origin::Host)),
    };
    for group in chain {
        let Some(defaults) = group.defaults.as_ref() else {
            continue;
        };
        let origin = Origin::Group(group.id);
        if login.username.is_none()
            && let Some(u) = non_empty(defaults.username.as_deref())
        {
            login.username = Some((u, origin));
        }
        if login.identity_id.is_none()
            && !host_answers_credentials
            && let Some(id) = defaults.identity_id
            && identities.iter().any(|i| i.id == id)
        {
            login.identity_id = Some((id, origin));
        }
    }
    if login.username.is_none()
        && let Some((iid, origin)) = login.identity_id
        && let Some(u) = identities
            .iter()
            .find(|i| i.id == iid)
            .and_then(|i| non_empty(i.username.as_deref()))
    {
        login.username = Some((u, origin));
    }
    login
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::group::GroupDefaults;

    fn with_defaults(label: &str, parent: Option<Uuid>, defaults: GroupDefaults) -> Group {
        let mut g = Group::new(label);
        g.parent_id = parent;
        g.defaults = Some(defaults);
        g
    }

    fn identity(username: Option<&str>) -> Identity {
        let mut i = Identity::new("login");
        i.username = username.map(str::to_string);
        i
    }

    fn host_in(group: &Group) -> Connection {
        let mut c = Connection::new("web", "example.com");
        c.group_id = Some(group.id);
        c
    }

    #[test]
    fn the_host_wins_over_every_group() {
        let g = with_defaults("prod", None, GroupDefaults { username: Some("deploy".into()), ..Default::default() });
        let mut conn = host_in(&g);
        conn.username = Some("me".into());
        let login = effective_login(&conn, &[&g], &[], false);
        assert_eq!(login.username, Some(("me".into(), Origin::Host)));
    }

    #[test]
    fn an_empty_username_is_unset() {
        let g = with_defaults("prod", None, GroupDefaults { username: Some("deploy".into()), ..Default::default() });
        let mut conn = host_in(&g);
        conn.username = Some(String::new());
        let login = effective_login(&conn, &[&g], &[], false);
        assert_eq!(login.username, Some(("deploy".into(), Origin::Group(g.id))));
    }

    #[test]
    fn the_nearest_group_wins_over_a_farther_one() {
        let root = with_defaults("root", None, GroupDefaults { username: Some("from-root".into()), ..Default::default() });
        let mid = with_defaults("mid", Some(root.id), GroupDefaults { username: Some("from-mid".into()), ..Default::default() });
        let conn = host_in(&mid);
        let login = effective_login(&conn, &[&mid, &root], &[], false);
        assert_eq!(login.username, Some(("from-mid".into(), Origin::Group(mid.id))));
    }

    /// The username walks the whole chain BEFORE any identity is asked:
    /// a group that names a user, however far up, is a more direct
    /// answer than a nearer group's identity.
    #[test]
    fn a_farther_username_beats_a_nearer_identity() {
        let ident = identity(Some("from-identity"));
        let root = with_defaults("root", None, GroupDefaults { username: Some("from-root".into()), ..Default::default() });
        let mid = with_defaults("mid", Some(root.id), GroupDefaults { identity_id: Some(ident.id), ..Default::default() });
        let conn = host_in(&mid);
        let login = effective_login(&conn, &[&mid, &root], std::slice::from_ref(&ident), false);
        assert_eq!(login.username, Some(("from-root".into(), Origin::Group(root.id))));
        assert_eq!(login.identity_id, Some((ident.id, Origin::Group(mid.id))));
    }

    #[test]
    fn the_hosts_own_identity_supplies_the_username() {
        let ident = identity(Some("from-identity"));
        let conn = {
            let mut c = Connection::new("web", "example.com");
            c.identity_id = Some(ident.id);
            c
        };
        let login = effective_login(&conn, &[], std::slice::from_ref(&ident), true);
        assert_eq!(login.username, Some(("from-identity".into(), Origin::Host)));
        assert_eq!(login.identity_id, Some((ident.id, Origin::Host)));
    }

    #[test]
    fn an_inherited_identity_supplies_the_username_with_its_origin() {
        let ident = identity(Some("from-identity"));
        let g = with_defaults("prod", None, GroupDefaults { identity_id: Some(ident.id), ..Default::default() });
        let conn = host_in(&g);
        let login = effective_login(&conn, &[&g], std::slice::from_ref(&ident), false);
        assert_eq!(login.username, Some(("from-identity".into(), Origin::Group(g.id))));
        assert_eq!(login.identity_id, Some((ident.id, Origin::Group(g.id))));
    }

    #[test]
    fn a_host_with_its_own_credentials_blocks_the_group_identity() {
        let ident = identity(Some("from-identity"));
        let g = with_defaults("prod", None, GroupDefaults { identity_id: Some(ident.id), ..Default::default() });
        let conn = host_in(&g);
        let login = effective_login(&conn, &[&g], &[ident], true);
        assert_eq!(login.identity_id, None);
        assert_eq!(login.username, None);
        assert_eq!(login.username_or_default(), DEFAULT_USERNAME);
    }

    #[test]
    fn a_dangling_group_identity_is_skipped() {
        let g = with_defaults("prod", None, GroupDefaults { identity_id: Some(Uuid::new_v4()), ..Default::default() });
        let conn = host_in(&g);
        let login = effective_login(&conn, &[&g], &[], false);
        assert_eq!(login.identity_id, None);
    }

    /// The host's own identity is not gated and not checked for
    /// existence here: naming one IS answering the credential, and a
    /// missing one is the credential resolver's problem, not a reason
    /// to inherit another.
    #[test]
    fn the_hosts_own_identity_is_untouched_by_the_gate() {
        let own = identity(None);
        let g = with_defaults("prod", None, GroupDefaults { identity_id: Some(Uuid::new_v4()), ..Default::default() });
        let mut conn = host_in(&g);
        conn.identity_id = Some(own.id);
        let login = effective_login(&conn, &[&g], std::slice::from_ref(&own), true);
        assert_eq!(login.identity_id, Some((own.id, Origin::Host)));
        assert_eq!(login.username, None);
    }
}
