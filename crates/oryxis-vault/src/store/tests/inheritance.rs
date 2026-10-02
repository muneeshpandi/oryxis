//! D4 resolution: what a host's settings become once its groups have
//! had their say.

use super::*;
use crate::store::inheritance::Origin;

/// `root` -> `mid` -> `leaf`, so "nearest ancestor wins" has something
/// to be nearest to.
fn chain() -> (Group, Group, Group) {
    let root = Group::new("root");
    let mut mid = Group::new("mid");
    mid.parent_id = Some(root.id);
    let mut leaf = Group::new("leaf");
    leaf.parent_id = Some(mid.id);
    (root, mid, leaf)
}

fn with_username(group: &mut Group, username: &str) {
    group.defaults = Some(GroupDefaults {
        username: Some(username.to_string()),
        ..Default::default()
    });
}

#[test]
fn the_host_wins_over_every_group() {
    let vault = temp_vault();
    let (root, mut mid, leaf) = chain();
    with_username(&mut mid, "from-group");
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(leaf.id);
    conn.username = Some("from-host".to_string());

    let effective = vault
        .resolve_effective(&conn, &[root, mid, leaf], &vault.list_identities().unwrap())
        .unwrap();
    assert_eq!(
        effective.username,
        Some(("from-host".to_string(), Origin::Host))
    );
}

#[test]
fn the_nearest_ancestor_wins_over_a_farther_one() {
    let vault = temp_vault();
    let (mut root, mut mid, leaf) = chain();
    with_username(&mut root, "from-root");
    with_username(&mut mid, "from-mid");
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(leaf.id);

    let effective = vault
        .resolve_effective(&conn, &[root, mid.clone(), leaf], &vault.list_identities().unwrap())
        .unwrap();
    assert_eq!(
        effective.username,
        Some(("from-mid".to_string(), Origin::Group(mid.id)))
    );
}

#[test]
fn a_field_no_one_sets_stays_unset() {
    let vault = temp_vault();
    let (root, mid, leaf) = chain();
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(leaf.id);

    let effective = vault
        .resolve_effective(&conn, &[root, mid, leaf], &vault.list_identities().unwrap())
        .unwrap();
    assert!(effective.username.is_none());
    assert!(effective.terminal_theme.is_none());
    assert!(effective.identity_id.is_none());
}

/// Each field walks the chain on its own, so a group setting one thing
/// does not stop a farther ancestor from answering for another.
#[test]
fn fields_resolve_independently_of_each_other() {
    let vault = temp_vault();
    let (mut root, mut mid, leaf) = chain();
    root.defaults = Some(GroupDefaults {
        terminal_theme: Some("nord".to_string()),
        ..Default::default()
    });
    with_username(&mut mid, "deploy");
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(leaf.id);

    let effective = vault
        .resolve_effective(&conn, &[root.clone(), mid.clone(), leaf], &vault.list_identities().unwrap())
        .unwrap();
    assert_eq!(
        effective.username,
        Some(("deploy".to_string(), Origin::Group(mid.id)))
    );
    assert_eq!(
        effective.terminal_theme,
        Some(("nord".to_string(), Origin::Group(root.id)))
    );
}

/// Env vars merge by name instead of one list replacing the other: a
/// host that defines one variable must not silently lose the group's.
#[test]
fn env_vars_merge_by_name_with_the_host_winning() {
    let vault = temp_vault();
    let (mut root, mut mid, leaf) = chain();
    root.defaults = Some(GroupDefaults {
        env_vars: vec![
            EnvVar { key: "TERM".into(), value: "xterm".into() },
            EnvVar { key: "ROOT_ONLY".into(), value: "1".into() },
        ],
        ..Default::default()
    });
    mid.defaults = Some(GroupDefaults {
        env_vars: vec![EnvVar { key: "TERM".into(), value: "screen".into() }],
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(leaf.id);
    conn.env_vars = vec![EnvVar { key: "HOST_ONLY".into(), value: "yes".into() }];

    let effective = vault
        .resolve_effective(&conn, &[root, mid, leaf], &vault.list_identities().unwrap())
        .unwrap();
    let get = |k: &str| {
        effective
            .env_vars
            .iter()
            .find(|e| e.key == k)
            .map(|e| e.value.clone())
    };
    // Nearer scope overrides the farther one for the same name...
    assert_eq!(get("TERM").as_deref(), Some("screen"));
    // ...and everything else from every scope survives.
    assert_eq!(get("ROOT_ONLY").as_deref(), Some("1"));
    assert_eq!(get("HOST_ONLY").as_deref(), Some("yes"));
    assert_eq!(effective.env_vars.len(), 3);
}

#[test]
fn a_host_env_var_overrides_the_group_one() {
    let vault = temp_vault();
    let (root, mut mid, leaf) = chain();
    mid.defaults = Some(GroupDefaults {
        env_vars: vec![EnvVar { key: "TERM".into(), value: "screen".into() }],
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(leaf.id);
    conn.env_vars = vec![EnvVar { key: "TERM".into(), value: "xterm-256color".into() }];

    let effective = vault
        .resolve_effective(&conn, &[root, mid, leaf], &vault.list_identities().unwrap())
        .unwrap();
    assert_eq!(effective.env_vars.len(), 1);
    assert_eq!(effective.env_vars[0].value, "xterm-256color");
}

/// A parent loop is data two synced devices can produce (each
/// re-parents one of a pair, LWW keeps both edges). The walk has to
/// terminate, and the host's own fields must still resolve: a corrupt
/// hierarchy is not a reason a host cannot connect.
#[test]
fn a_parent_cycle_terminates_and_still_resolves() {
    let vault = temp_vault();
    let mut a = Group::new("a");
    let mut b = Group::new("b");
    a.parent_id = Some(b.id);
    b.parent_id = Some(a.id);
    with_username(&mut b, "from-b");
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(a.id);
    conn.terminal_theme = Some("dracula".to_string());

    let effective = vault.resolve_effective(&conn, &[a, b.clone()], &vault.list_identities().unwrap()).unwrap();
    // The host's own value is never at the mercy of the hierarchy.
    assert_eq!(
        effective.terminal_theme,
        Some(("dracula".to_string(), Origin::Host))
    );
    // And the loop's members are still walked once each, so what they
    // do set is honored rather than lost.
    assert_eq!(
        effective.username,
        Some(("from-b".to_string(), Origin::Group(b.id)))
    );
}

/// Mid-sync an ancestor's own record may not have arrived yet. That is
/// transient and must not fail the resolution.
#[test]
fn a_dangling_parent_stops_the_walk_without_failing() {
    let vault = temp_vault();
    let mut leaf = Group::new("leaf");
    leaf.parent_id = Some(Uuid::new_v4());
    with_username(&mut leaf, "from-leaf");
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(leaf.id);

    let effective = vault.resolve_effective(&conn, &[leaf.clone()], &vault.list_identities().unwrap()).unwrap();
    assert_eq!(
        effective.username,
        Some(("from-leaf".to_string(), Origin::Group(leaf.id)))
    );
}

#[test]
fn a_host_in_no_group_resolves_to_its_own_fields() {
    let vault = temp_vault();
    let (root, mut mid, leaf) = chain();
    with_username(&mut mid, "unreachable");
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = None;
    conn.username = Some("solo".to_string());

    let effective = vault
        .resolve_effective(&conn, &[root, mid, leaf], &vault.list_identities().unwrap())
        .unwrap();
    assert_eq!(
        effective.username,
        Some(("solo".to_string(), Origin::Host))
    );
}

/// A group pointing at a proxy identity the user deleted must leave the
/// host proxy-less, never error: the same forgiveness `resolve_proxy`
/// already gives a host with a dangling reference.
#[test]
fn a_dangling_group_proxy_identity_resolves_to_none() {
    let vault = unlocked_vault();
    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        proxy_identity_id: Some(Uuid::new_v4()),
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);

    let effective = vault.resolve_effective(&conn, &[group], &vault.list_identities().unwrap()).unwrap();
    assert!(effective.proxy.is_none());
}

/// The host's own proxy keeps precedence, and it keeps it through
/// `resolve_proxy` rather than a second implementation of the same
/// rules.
#[test]
fn a_host_proxy_wins_over_the_group_default() {
    let vault = unlocked_vault();
    let host_ident = oryxis_core::models::proxy_identity::ProxyIdentity {
        id: Uuid::new_v4(),
        label: "host-proxy".into(),
        proxy_type: ProxyType::Socks5,
        host: "host-proxy.internal".into(),
        port: 1080,
        username: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let group_ident = oryxis_core::models::proxy_identity::ProxyIdentity {
        id: Uuid::new_v4(),
        label: "group-proxy".into(),
        proxy_type: ProxyType::Socks5,
        host: "group-proxy.internal".into(),
        port: 1080,
        username: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    vault.save_proxy_identity(&host_ident, None).unwrap();
    vault.save_proxy_identity(&group_ident, None).unwrap();

    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        proxy_identity_id: Some(group_ident.id),
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);
    conn.proxy_identity_id = Some(host_ident.id);

    let effective = vault.resolve_effective(&conn, &[group], &vault.list_identities().unwrap()).unwrap();
    let (proxy, origin) = effective.proxy.expect("a proxy resolved");
    assert_eq!(proxy.host, "host-proxy.internal");
    assert_eq!(origin, Origin::Host);
}

#[test]
fn the_group_proxy_applies_when_the_host_has_none() {
    let vault = unlocked_vault();
    let ident = oryxis_core::models::proxy_identity::ProxyIdentity {
        id: Uuid::new_v4(),
        label: "group-proxy".into(),
        proxy_type: ProxyType::Socks5,
        host: "group-proxy.internal".into(),
        port: 1080,
        username: Some("bastion".into()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    vault.save_proxy_identity(&ident, Some("s3cret")).unwrap();

    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        proxy_identity_id: Some(ident.id),
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);

    let effective = vault.resolve_effective(&conn, &[group.clone()], &vault.list_identities().unwrap()).unwrap();
    let (proxy, origin) = effective.proxy.expect("the group's proxy resolved");
    assert_eq!(proxy.host, "group-proxy.internal");
    assert_eq!(proxy.username.as_deref(), Some("bastion"));
    // Hydrated like the host path does it, password included.
    assert_eq!(proxy.password.as_deref(), Some("s3cret"));
    assert_eq!(origin, Origin::Group(group.id));
}

/// A group identity default reaches a host that answers nothing itself:
/// no username, no key, no stored password.
#[test]
fn the_group_identity_applies_to_a_bare_host() {
    let vault = unlocked_vault();
    let ident = oryxis_core::models::Identity::new("shared-login");
    vault.save_identity(&ident, Some("id-pw")).unwrap();

    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        identity_id: Some(ident.id),
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);

    let effective = vault.resolve_effective(&conn, &[group.clone()], &vault.list_identities().unwrap()).unwrap();
    assert_eq!(effective.identity_id, Some((ident.id, Origin::Group(group.id))));
}

/// A group default naming a deleted identity must fall through: the
/// engine's credential resolution takes the identity branch wholesale,
/// so a dangling reference would resolve to NO credentials at all and
/// eclipse the host's own. Mirrors the dangling proxy-identity rule and
/// the editor hint, which already skips it.
#[test]
fn a_dangling_group_identity_default_is_not_inherited() {
    let vault = unlocked_vault();
    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        identity_id: Some(Uuid::new_v4()),
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);

    let effective = vault.resolve_effective(&conn, &[group], &vault.list_identities().unwrap()).unwrap();
    assert!(effective.identity_id.is_none());
}

/// Credentials are ONE parameter family: a host that stores its own
/// password has answered it, so a group gaining an identity default
/// must not silently change how that host authenticates.
#[test]
fn a_host_with_its_own_password_blocks_the_group_identity() {
    let vault = unlocked_vault();
    let ident = oryxis_core::models::Identity::new("shared-login");
    vault.save_identity(&ident, Some("id-pw")).unwrap();

    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        identity_id: Some(ident.id),
        ..Default::default()
    });
    vault.save_group(&group).unwrap();
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);
    vault.save_connection(&conn, Some("host-pw")).unwrap();

    let effective = vault.resolve_effective(&conn, &[group], &vault.list_identities().unwrap()).unwrap();
    assert!(effective.identity_id.is_none());
}

/// Same family rule for a host that names its own key.
#[test]
fn a_host_with_its_own_key_blocks_the_group_identity() {
    let vault = unlocked_vault();
    let ident = oryxis_core::models::Identity::new("shared-login");
    vault.save_identity(&ident, Some("id-pw")).unwrap();

    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        identity_id: Some(ident.id),
        ..Default::default()
    });
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);
    conn.key_id = Some(Uuid::new_v4());

    let effective = vault.resolve_effective(&conn, &[group], &vault.list_identities().unwrap()).unwrap();
    assert!(effective.identity_id.is_none());
}

/// The host's own identity always wins over the family gate: naming an
/// identity IS answering the credential parameter.
#[test]
fn the_hosts_own_identity_is_untouched_by_the_gate() {
    let vault = unlocked_vault();
    let own = oryxis_core::models::Identity::new("own-login");
    vault.save_identity(&own, Some("own-pw")).unwrap();

    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        identity_id: Some(Uuid::new_v4()),
        ..Default::default()
    });
    vault.save_group(&group).unwrap();
    let mut conn = Connection::new("web", "example.com");
    conn.group_id = Some(group.id);
    conn.identity_id = Some(own.id);
    vault.save_connection(&conn, Some("host-pw")).unwrap();

    let effective = vault.resolve_effective(&conn, &[group], &vault.list_identities().unwrap()).unwrap();
    assert_eq!(effective.identity_id, Some((own.id, Origin::Host)));
}
