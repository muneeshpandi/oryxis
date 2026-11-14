//! Where a snapshot transport syncs, as one comparable key, and whether
//! another vault already syncs there (`MULTI_VAULT_SPEC.md` § 8).
//!
//! Two vaults pointed at the same folder, Git remote, WebDAV URL or SFTP
//! file would merge into each other: each round reads the other vault's
//! records out of the shared snapshot and writes them into its own. So
//! a round (and turning sync on) is refused while another registered
//! vault with sync ON uses the same target, read from that vault's file
//! when it is not open (the settings are plaintext, readable locked).

use uuid::Uuid;

/// The setting rows a vault's target is read from.
const TARGET_KEYS: &[&str] = &[
    "sync_enabled",
    "sync_transport",
    "sync_folder_path",
    "sync_git_remote",
    "sync_git_folder",
    "sync_webdav_url",
    "sync_sftp_host_id",
    "sync_sftp_remote_path",
];

/// What decides a snapshot transport's target, as configured.
#[derive(Debug, Default, Clone)]
pub(crate) struct TargetConfig {
    pub transport: String,
    pub folder_path: String,
    pub git_remote: String,
    pub git_folder: String,
    pub webdav_url: String,
    /// The SFTP host's address (hostname, port), resolved from the host
    /// the vault names (host ids are per vault, addresses are not).
    pub sftp_host: Option<(String, u16)>,
    pub sftp_remote_path: String,
}

impl TargetConfig {
    /// The comparable key of where this configuration syncs; `None` for
    /// P2P (no shared file) or an incomplete configuration.
    pub(crate) fn key(&self) -> Option<String> {
        let trimmed = |s: &str| s.trim().trim_end_matches(['/', '\\']).to_string();
        let key = match self.transport.as_str() {
            "folder" => {
                let raw = self.folder_path.trim();
                if raw.is_empty() {
                    return None;
                }
                // The snapshot FILE the round would write, `~` expanded
                // and the folder canonical when it exists.
                let file = crate::dispatch_folder_sync::snapshot_path(raw);
                let path = match (file.parent().and_then(|p| std::fs::canonicalize(p).ok()), file.file_name()) {
                    (Some(dir), Some(name)) => dir.join(name),
                    _ => file,
                }
                .display()
                .to_string();
                // Case folds where the file system does.
                let path = if cfg!(any(windows, target_os = "macos")) { path.to_lowercase() } else { path };
                format!("folder:{path}")
            }
            "git" => {
                let remote = trimmed(&self.git_remote);
                let remote = remote.strip_suffix(".git").unwrap_or(&remote).to_string();
                if remote.is_empty() {
                    return None;
                }
                match crate::dispatch_git_sync::clean_folder(&self.git_folder) {
                    Some(f) if !f.is_empty() => format!("git:{remote}#{f}"),
                    _ => format!("git:{remote}"),
                }
            }
            "webdav" => {
                let url = trimmed(&self.webdav_url);
                if url.is_empty() {
                    return None;
                }
                format!("webdav:{url}")
            }
            "sftp" => {
                let (host, port) = self.sftp_host.as_ref()?;
                let path = trimmed(&self.sftp_remote_path);
                if path.is_empty() {
                    return None;
                }
                format!("sftp:{}:{port}:{path}", host.trim().to_lowercase())
            }
            _ => return None,
        };
        Some(key)
    }

    /// The configuration a vault FILE holds, and whether its sync is on.
    fn of_file(path: &std::path::Path) -> (bool, Self) {
        let rows = oryxis_vault::device::read_plain_settings(path, TARGET_KEYS);
        let get = |k: &str| rows.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let sftp_host = Uuid::parse_str(&get("sync_sftp_host_id"))
            .ok()
            .and_then(|id| oryxis_vault::device::read_host_address(path, id));
        let config = Self {
            transport: get("sync_transport"),
            folder_path: get("sync_folder_path"),
            git_remote: get("sync_git_remote"),
            git_folder: get("sync_git_folder"),
            webdav_url: get("sync_webdav_url"),
            sftp_host,
            sftp_remote_path: get("sync_sftp_remote_path"),
        };
        (get("sync_enabled") == "true", config)
    }
}

impl crate::app::Oryxis {
    /// The acting vault's snapshot target, as configured now.
    fn sync_target_here(&self) -> TargetConfig {
        let sync = &self.vaults.cur().sync;
        TargetConfig {
            transport: sync.transport.clone(),
            folder_path: sync.folder.path.clone(),
            git_remote: sync.git.remote.clone(),
            git_folder: sync.git.folder.clone(),
            webdav_url: sync.webdav.url.clone(),
            sftp_host: sync.sftp.host_id.and_then(|id| {
                self.vaults.cur().connections.iter().find(|c| c.id == id).map(|c| (c.hostname.clone(), c.port))
            }),
            sftp_remote_path: sync.sftp.remote_path.clone(),
        }
    }

    /// The name of another registered vault whose sync is on and uses the
    /// acting vault's target, if any.
    pub(crate) fn sync_target_taken_by(&self) -> Option<String> {
        let here = self.sync_target_here().key()?;
        let device = oryxis_vault::device::installed()?;
        let me = self.vaults.current_id();
        self.registered_vaults().into_iter().filter(|v| v.id != me).find_map(|v| {
            let (enabled, config) = TargetConfig::of_file(&device.resolve(&v));
            (enabled && config.key().as_deref() == Some(here.as_str())).then(|| self.vault_name(v.id))
        })
    }

    /// The refusal text for a target another vault syncs with.
    pub(crate) fn sync_target_taken_text(vault: &str) -> String {
        crate::i18n::t("sync_target_taken").replacen("{vault}", vault, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(transport: &str) -> TargetConfig {
        TargetConfig { transport: transport.into(), ..Default::default() }
    }

    #[test]
    fn the_same_target_spelled_two_ways_is_one_key() {
        let mut a = config("git");
        a.git_remote = "git@github.com:me/vault.git".into();
        let mut b = config("git");
        b.git_remote = " git@github.com:me/vault/ ".into();
        assert_eq!(a.key(), b.key());

        let mut a = config("webdav");
        a.webdav_url = "https://dav.example.com/oryxis/".into();
        let mut b = config("webdav");
        b.webdav_url = "https://dav.example.com/oryxis".into();
        assert_eq!(a.key(), b.key());

        let mut a = config("sftp");
        a.sftp_host = Some(("NAS.lan".into(), 22));
        a.sftp_remote_path = "/backup/oryxis/".into();
        let mut b = config("sftp");
        b.sftp_host = Some(("nas.lan".into(), 22));
        b.sftp_remote_path = "/backup/oryxis".into();
        assert_eq!(a.key(), b.key());
        b.sftp_host = Some(("nas.lan".into(), 2222));
        assert_ne!(a.key(), b.key(), "another port is another server");
    }

    #[test]
    fn different_transports_and_p2p_never_clash() {
        let mut folder = config("folder");
        folder.folder_path = "/data/x".into();
        let mut webdav = config("webdav");
        webdav.webdav_url = "/data/x".into();
        assert_ne!(folder.key(), webdav.key());
        assert_eq!(config("p2p").key(), None);
        assert_eq!(config("folder").key(), None, "an empty path is no target");
    }

    #[test]
    fn a_git_folder_makes_a_distinct_target() {
        let base = TargetConfig { transport: "git".into(), git_remote: "git@h:me/v.git".into(), ..Default::default() };
        let a = TargetConfig { git_folder: "alpha".into(), ..base.clone() };
        let b = TargetConfig { git_folder: "beta".into(), ..base.clone() };
        assert_ne!(a.key(), b.key());
        assert_ne!(a.key(), base.key());
        assert_eq!(base.key(), Some("git:git@h:me/v".to_string()));
    }
}
