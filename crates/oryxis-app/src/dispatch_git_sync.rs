//! Git sync transport: the same encrypted snapshot, committed to a Git
//! remote.
//!
//! Why this exists next to the folder transport, which already covers
//! every cloud that mounts a directory: Git is the only backend that
//! keeps HISTORY. Every round is a commit, so a vault that got wrecked
//! (a bad import, a deletion synced from the wrong device) can be read
//! back from an earlier one. No other transport can offer that, because
//! they all store exactly one blob.
//!
//! It also gives the strongest conflict detection we have. A push
//! rejected as non-fast-forward means another device wrote since our
//! fetch, which is a real compare-and-swap; the SFTP and folder
//! transports can only self-heal after the fact.
//!
//! It drives the `git` BINARY rather than linking a Git library. Owner
//! call: the audience installs git, and documenting that requirement is
//! cheaper than carrying `gix` or `git2` in every build for one
//! backend.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::Task;

use oryxis_vault::VaultStore;

use crate::app::{Message, Oryxis, SyncMessage};
use crate::dispatch_sync::SyncRoundTrigger;
use crate::i18n::t;

/// Snapshot filename inside the repository. Same name the other
/// transports use, so the blob is recognisably the same artifact.
const SNAPSHOT_NAME: &str = "oryxis-sync.bin";

/// Sidecar holding the fingerprint of the vault the snapshot was built
/// from.
///
/// It exists because the snapshot bytes cannot be compared: the payload
/// is sealed with a fresh nonce each time, so an unchanged vault still
/// produces a different file, and committing "when the file changed"
/// would put an empty commit in the history on every single round. That
/// would bury the real ones, and history is the entire reason to use
/// this transport. The sidecar is a hash of ids and timestamps, so it
/// discloses nothing the snapshot does not already hold in sealed form.
const SIGNATURE_NAME: &str = "oryxis-sync.sig";

/// How many times a rejected push is retried before giving up.
///
/// A rejection means someone else pushed between our fetch and our
/// push, so the fix is to redo the round on top of theirs. Three is
/// generous: each attempt starts from a fresh fetch, and a device that
/// loses three races in a row is contending with something that will
/// still be there in a minute. We never force-push, because that would
/// discard the other device's edits, which is the exact outcome the
/// whole design exists to prevent.
/// Rounds are serialised process-wide: vaults on the same remote share
/// one working clone, and two rounds resetting and committing in it at
/// once would corrupt each other.
static ROUND_LOCK: Mutex<()> = Mutex::new(());

/// A safe folder name from free text: lowercase letters, digits, `-`
/// and `_`, everything else collapsed to `-`.
pub(crate) fn folder_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() { "vault".to_string() } else { s }
}

/// Validate a typed folder. `Some("")` is the repository root. `None`
/// means unusable (traversal, absolute path, hidden or option-like
/// segment). Nested `a/b` is allowed; `\` is read as `/`.
pub(crate) fn clean_folder(raw: &str) -> Option<String> {
    let norm = raw.trim().replace('\\', "/");
    let mut parts = Vec::new();
    for seg in norm.split('/') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if seg.starts_with('.') || seg.starts_with('-') || seg.contains(':') {
            return None;
        }
        parts.push(seg.to_string());
    }
    if norm.starts_with('/') {
        return None;
    }
    Some(parts.join("/"))
}

const PUSH_ATTEMPTS: usize = 3;

/// Per-command budget.
///
/// Generous enough for a clone/push over any connection a sync targets,
/// short enough that a black-holed network (TCP stalls are minutes, not
/// seconds) cannot wedge the transport: a round that hangs would leave
/// `in_progress` set forever and silently kill every later round, auto
/// and manual alike. The WebDAV transport caps its requests at 60s and
/// SFTP at its per-op setting; this is the git equivalent.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Where the working copies live: one clone per remote, under the
/// app's own directory.
///
/// This is a CACHE, not user data. It can be deleted at any time and
/// the next round re-clones it; a corrupt clone is repaired the same
/// way rather than reported.
fn clone_root() -> PathBuf {
    oryxis_core::paths::oryxis_dir()
        .unwrap_or_else(|| PathBuf::from(".").join(".oryxis"))
        .join("sync-git")
}

/// A stable directory name for a remote URL. Hashed rather than
/// sanitised: a URL can carry characters no filesystem wants, and the
/// name only has to be unique and repeatable.
fn clone_dir(remote: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    remote.trim().hash(&mut h);
    clone_root().join(format!("{:016x}", h.finish()))
}

/// Run a git command in `dir`, returning stdout on success.
///
/// The environment is what keeps a sync round from hanging forever:
/// `git` asks for credentials and host-key confirmations on a terminal
/// that does not exist here, and would block until the app is killed.
/// `GIT_TERMINAL_PROMPT=0` and ssh's `BatchMode=yes` turn every such
/// question into a fast failure the user can actually be told about.
///
/// `COMMAND_TIMEOUT` is the backstop for the questions the environment
/// cannot answer fast: a stalled network. `output()` would block until
/// the OS gives up on the socket (minutes), so the child is spawned
/// with piped stdio and polled with `try_wait` while two reader threads
/// drain stdout/stderr (a full pipe would stall the child the same
/// way). On timeout the child is killed and the round reports it.
fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env(
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        );
    // CREATE_NO_WINDOW (0x0800_0000): this binary is GUI-subsystem, so
    // every `git` spawn would otherwise pop a visible console window
    // over the app (same guard wsl.exe / the plugin hosts / the
    // updater use). One flag keeps a whole round from flashing.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000);
    }
    // `spawn()` inherits stdin (unlike the old `output()`, which nulled
    // it): null it explicitly so a git that decides to read stdin
    // anyway fails fast instead of idling until the timeout kills it.
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out.join();
                    let _ = err.join();
                    return Err(t("git_sync_timeout").to_string());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    } else {
        // git says everything useful on stderr.
        let err = String::from_utf8_lossy(&stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("git {} failed", args.first().copied().unwrap_or(""))
        } else {
            err
        })
    }
}

/// Whether a usable `git` is on PATH. Checked when the transport is
/// selected, not on the first sync, so the answer arrives while the
/// user is still looking at the setting.
pub(crate) fn git_available() -> bool {
    git(None, &["--version"]).is_ok()
}

/// Re-check `git_available` on a worker thread and cache the answer in
/// `GitSyncForm::git_available`.
///
/// The probe must never run inside `view()`: it spawns a subprocess,
/// so a per-render call blocks the UI thread and (on Windows) flashes
/// a console window every frame. Boot, a transport switch, opening the
/// Sync settings section and each finished round refresh the cache
/// instead; the section-open refresh is what keeps the card's "install
/// it and reopen this screen" instruction true.
pub(crate) fn git_availability_task() -> Task<Message> {
    Task::perform(
        // `spawn_blocking`, not a bare async block: the probe waits on
        // a subprocess (worst case the whole `COMMAND_TIMEOUT`), and a
        // bare block would pin an executor thread for that long.
        async move {
            tokio::task::spawn_blocking(git_available)
                .await
                .unwrap_or(false)
        },
        |available| Message::Sync(SyncMessage::GitAvailabilityChecked(available)),
    )
}

impl Oryxis {
    /// Run one Git-sync round.
    pub(crate) fn run_git_sync_round(&mut self, trigger: SyncRoundTrigger) -> Task<Message> {
        if self.vaults.cur().sync.git.in_progress {
            return Task::none();
        }
        // A locked vault has no master key to decrypt with; the
        // manual buttons must not rely on the lock screen hiding them.
        if !self.sync_round_allowed() {
            return Task::none();
        }
        // Another vault syncing with the same target would merge into
        // this one (`sync_targets`).
        if let Some(other) = self.sync_target_taken_by() {
            self.vaults.cur_mut().sync.git.status = Some(Err(Self::sync_target_taken_text(&other)));
            return Task::none();
        }
        let remote = self.vaults.cur().sync.git.remote.trim().to_string();
        if remote.is_empty() {
            self.vaults.cur_mut().sync.git.status = Some(Err(t("git_sync_no_remote").to_string()));
            return Task::none();
        }
        let Some(vault) = &self.vaults.cur().store else {
            return Task::none();
        };
        // Group key: the typed buffer for a manual round, else the
        // stored value. The four transports share one row and one edit
        // buffer, so there is no stale-form drift to guard against; what
        // still has to be guarded is drift in TIME (an auto round never
        // reads the buffer) and IN FLIGHT (the key is armed below and
        // committed by value, never re-read from the field).
        let Some(key) = self.sync_round_passphrase(trigger) else {
            self.vaults.cur_mut().sync.git.status = Some(Err(t("sftp_sync_no_passphrase").to_string()));
            return Task::none();
        };
        // The Argon2id derivation (~0.4 s) runs inside the blocking
        // task, not on the UI thread: it used to freeze the app for
        // every "Sync now" click (the stall watchdog logged GitSyncNow
        // at ~370 ms per round).
        let db_path = vault.db_path().to_path_buf();
        let master_password = self.vaults.cur().master_password.clone();
        let folder = self.vaults.cur().sync.git.folder.clone();
        // Same fallback as `start_sync_engine`: an unset device name
        // would otherwise render as a bare "oryxis: sync from" commit.
        let device = if self.device_prefs.sync_device_name.trim().is_empty() {
            "oryxis-device".to_string()
        } else {
            self.device_prefs.sync_device_name.clone()
        };

        self.vaults.cur_mut().sync.git.in_progress = true;
        self.vaults.cur_mut().sync.git.status = None;
        // Remember what seals this snapshot, so the commit at the end
        // writes that key and not whatever the field holds by then.
        let passphrase = self.arm_round_passphrase(key);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let secret =
                        oryxis_vault::derive_sync_secret(&passphrase).map_err(|e| e.to_string())?;
                    git_round(&remote, &db_path, master_password, &secret, &device, &folder)
                })
                .await
                .unwrap_or_else(|e| Err(format!("join: {e}")))
            },
            |result| Message::Sync(SyncMessage::GitRoundFinished(result)),
        )
    }
}

/// Make sure `dir` holds a working clone of `remote`.
///
/// Anything unexpected (missing, not a repository, pointing somewhere
/// else) is repaired by starting over rather than reported: the clone
/// is a cache, and a user should never have to know it exists.
fn ensure_clone(remote: &str, dir: &Path) -> Result<(), String> {
    let healthy = dir.join(".git").is_dir()
        && git(Some(dir), &["remote", "get-url", "origin"])
            .map(|url| url.trim() == remote)
            .unwrap_or(false);
    if healthy {
        return Ok(());
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // `--` ends option parsing so a remote beginning with `-`
    // (`--upload-pack=…`, `-c…`) is treated as a URL, not a git flag.
    // The remote is user-typed and local, so this is defence in depth
    // rather than a live attacker path, but it costs one token.
    git(
        None,
        &["clone", "--", remote, dir.to_string_lossy().as_ref()],
    )
    .map(|_| ())
}

/// The branch the clone is on.
///
/// `symbolic-ref` rather than `rev-parse HEAD`, because cloning an
/// EMPTY remote is the normal first run and `rev-parse` fails there
/// ("ambiguous argument 'HEAD'") since no commit exists yet.
/// `symbolic-ref` still reports the initial branch name, which is
/// exactly what the first push needs to create.
fn current_branch(dir: &Path) -> Result<String, String> {
    if let Ok(name) = git(Some(dir), &["symbolic-ref", "--short", "HEAD"]) {
        let name = name.trim().to_string();
        if !name.is_empty() {
            return Ok(name);
        }
    }
    // Detached HEAD (nothing to push to): follow the remote's default.
    let head = git(Some(dir), &["symbolic-ref", "refs/remotes/origin/HEAD"]).unwrap_or_default();
    let fallback = head.trim().rsplit('/').next().unwrap_or("").to_string();
    Ok(if fallback.is_empty() {
        "main".to_string()
    } else {
        fallback
    })
}

/// One round: sync the vault through the remote, retrying on the races
/// that a rejected push reports.
fn git_round(
    remote: &str,
    db_path: &Path,
    master_password: Option<String>,
    secret: &[u8; 32],
    device: &str,
    folder: &str,
) -> Result<usize, String> {
    let folder = clean_folder(folder).ok_or_else(|| t("git_sync_bad_folder").to_string())?;
    let _guard = ROUND_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !git_available() {
        return Err(t("git_sync_no_git").to_string());
    }
    let dir = clone_dir(remote);
    ensure_clone(remote, &dir)?;
    let branch = current_branch(&dir)?;
    let base = if folder.is_empty() { dir.clone() } else { dir.join(&folder) };
    let rel = |name: &str| if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
    let snapshot_path = base.join(SNAPSHOT_NAME);

    // Unlocked ONCE, outside the retry loop, like the WebDAV round: the
    // unlock is an Argon2id derivation tuned to take about a second, and
    // a rejected push is a reason to re-merge, never a reason to
    // re-derive the key.
    let mut vault = VaultStore::open(db_path).map_err(|e| e.to_string())?;
    match master_password.as_deref() {
        Some(pw) => vault.unlock(pw).map_err(|e| e.to_string())?,
        None => vault.open_without_password().map_err(|e| e.to_string())?,
    }
    let vault = Arc::new(Mutex::new(vault));

    let mut last_err = String::new();
    for attempt in 0..PUSH_ATTEMPTS {
        // Start from whatever the remote has. `reset --hard` is safe
        // here precisely because the clone is a cache: the only file we
        // ever write is rebuilt from the vault below, so there is no
        // local work to lose.
        let _ = git(Some(&dir), &["fetch", "origin"]);
        let remote_ref = format!("origin/{branch}");
        if git(Some(&dir), &["rev-parse", "--verify", &remote_ref]).is_ok() {
            git(Some(&dir), &["reset", "--hard", &remote_ref])?;
        }
        // An empty repository (no commits yet) simply has nothing to
        // merge; the first push creates the branch.

        let mut pulled = 0usize;
        if snapshot_path.is_file() {
            let blob = std::fs::read(&snapshot_path).map_err(|e| e.to_string())?;
            pulled = oryxis_sync::merge_snapshot(&vault, &blob, secret)
                .map_err(|e| e.to_string())?;
        }
        // Compare the LOGICAL content, not the bytes: an unchanged
        // vault re-seals to a different blob every time.
        let signature = oryxis_sync::vault_signature(&vault).map_err(|e| e.to_string())?;
        let signature_path = base.join(SIGNATURE_NAME);
        let remote_signature = std::fs::read_to_string(&signature_path)
            .unwrap_or_default()
            .trim()
            .to_string();
        if remote_signature == signature {
            // The remote already describes exactly this vault. Nothing
            // to say, and saying it would be an empty commit.
            return Ok(pulled);
        }

        let rebuilt = oryxis_sync::build_full_snapshot(&vault, secret)
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        std::fs::write(&snapshot_path, &rebuilt).map_err(|e| e.to_string())?;
        std::fs::write(&signature_path, &signature).map_err(|e| e.to_string())?;

        git(Some(&dir), &["add", "--", &rel(SNAPSHOT_NAME), &rel(SIGNATURE_NAME)])?;
        if git(Some(&dir), &["diff", "--cached", "--quiet"]).is_ok() {
            return Ok(pulled);
        }
        let message = format!("oryxis: sync from {device}");
        git(
            Some(&dir),
            &[
                "-c",
                "user.name=Oryxis",
                "-c",
                "user.email=sync@oryxis.app",
                "commit",
                "-m",
                &message,
            ],
        )?;
        match git(Some(&dir), &["push", "origin", &format!("HEAD:{branch}")]) {
            Ok(_) => return Ok(pulled),
            Err(e) => {
                last_err = e;
                // Another device pushed in between. Redo the round on
                // top of theirs; never force, which would delete it.
                tracing::info!(
                    attempt = attempt + 1,
                    "git sync push rejected, retrying on top of the remote"
                );
            }
        }
    }
    Err(t("git_sync_busy")
        .replace("{error}", &last_err))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clone directory is derived from the URL, stable across runs
    /// and different per remote.
    #[test]
    fn each_remote_gets_its_own_stable_directory() {
        let a = clone_dir("git@example.com:me/vault.git");
        let b = clone_dir("git@example.com:me/other.git");
        assert_ne!(a, b);
        assert_eq!(a, clone_dir("git@example.com:me/vault.git"));
        // Whitespace around the URL is the same remote.
        assert_eq!(a, clone_dir("  git@example.com:me/vault.git  "));
    }

    /// A local bare repository is a real remote, which makes the whole
    /// round testable without a network or a forge account.
    #[test]
    fn a_round_commits_the_snapshot_and_a_second_vault_reads_it() {
        if !git_available() {
            eprintln!("git not installed, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        git(None, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()])
            .expect("bare remote created");
        let remote_url = remote.to_string_lossy().into_owned();

        let vault_a = dir.path().join("a.db");
        {
            let mut v = VaultStore::open(&vault_a).unwrap();
            v.set_master_password("pw").unwrap();
            let conn = oryxis_core::models::Connection::new("web", "example.com");
            v.save_connection(&conn, None).unwrap();
        }
        let secret = oryxis_vault::derive_sync_secret("group").unwrap();
        let pulled = git_round(&remote_url, &vault_a, Some("pw".into()), &secret, "device-a", "")
            .expect("first round");
        assert_eq!(pulled, 0, "nothing to pull on the first round");

        // The remote now carries a commit with the snapshot in it.
        let log = git(None, &["--git-dir", remote.to_str().unwrap(), "log", "--oneline"])
            .expect("remote has history");
        assert!(log.contains("oryxis: sync from device-a"), "log: {log}");

        // A second vault through the same remote absorbs the first's host.
        let vault_b = dir.path().join("b.db");
        {
            let mut v = VaultStore::open(&vault_b).unwrap();
            v.set_master_password("pw2").unwrap();
        }
        let pulled = git_round(&remote_url, &vault_b, Some("pw2".into()), &secret, "device-b", "")
            .expect("second round");
        assert!(pulled > 0, "the second device merged the first one's records");
        let mut v = VaultStore::open(&vault_b).unwrap();
        v.unlock("pw2").unwrap();
        assert!(v.list_connections().unwrap().iter().any(|c| c.label == "web"));
    }

    /// Running twice with no changes must not pile up empty commits:
    /// history is the feature, and a commit per idle round would bury
    /// the real ones.
    #[test]
    fn an_unchanged_vault_makes_no_commit() {
        if !git_available() {
            eprintln!("git not installed, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        git(None, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]).unwrap();
        let remote_url = remote.to_string_lossy().into_owned();
        let vault = dir.path().join("v.db");
        {
            let mut v = VaultStore::open(&vault).unwrap();
            v.set_master_password("pw").unwrap();
        }
        let secret = oryxis_vault::derive_sync_secret("group").unwrap();
        git_round(&remote_url, &vault, Some("pw".into()), &secret, "d", "").unwrap();
        let before = git(None, &["--git-dir", remote.to_str().unwrap(), "rev-list", "--count", "HEAD"]).unwrap();
        git_round(&remote_url, &vault, Some("pw".into()), &secret, "d", "").unwrap();
        let after = git(None, &["--git-dir", remote.to_str().unwrap(), "rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(before.trim(), after.trim(), "an idle round committed anyway");
    }

    /// A clone that has been deleted, or that points somewhere else, is
    /// repaired rather than reported: it is a cache.
    #[test]
    fn a_broken_clone_is_re_cloned() {
        if !git_available() {
            eprintln!("git not installed, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        git(None, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]).unwrap();
        let remote_url = remote.to_string_lossy().into_owned();
        let work = dir.path().join("work");
        // Not a repository at all.
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join("junk.txt"), b"not a repo").unwrap();
        ensure_clone(&remote_url, &work).expect("repaired");
        assert!(work.join(".git").is_dir());
        assert!(!work.join("junk.txt").exists(), "the old contents went away");
    }

    #[test]
    fn slug_and_folder_validation() {
        assert_eq!(folder_slug("My Vault!"), "my-vault");
        assert_eq!(folder_slug("  "), "vault");
        assert_eq!(clean_folder(""), Some(String::new()));
        assert_eq!(clean_folder(" work/prod "), Some("work/prod".into()));
        assert_eq!(clean_folder("../x"), None);
        assert_eq!(clean_folder("/abs"), None);
        assert_eq!(clean_folder("-flag"), None);
        assert_eq!(clean_folder(".git"), None);
    }

    /// Two vaults, one repository, different folders: neither merges
    /// into the other, and the legacy root layout is untouched.
    #[test]
    fn two_vaults_share_one_repo_in_separate_folders() {
        if !git_available() {
            eprintln!("git not installed, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        git(None, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]).unwrap();
        let url = remote.to_string_lossy().into_owned();
        let mk = |name: &str, pw: &str, label: &str| {
            let p = dir.path().join(name);
            let mut v = VaultStore::open(&p).unwrap();
            v.set_master_password(pw).unwrap();
            v.save_connection(&oryxis_core::models::Connection::new(label, "h.example"), None).unwrap();
            p
        };
        let a = mk("a.db", "pa", "only-in-a");
        let b = mk("b.db", "pb", "only-in-b");
        let secret = oryxis_vault::derive_sync_secret("group").unwrap();
        git_round(&url, &a, Some("pa".into()), &secret, "d", "alpha").unwrap();
        let pulled = git_round(&url, &b, Some("pb".into()), &secret, "d", "beta").unwrap();
        assert_eq!(pulled, 0, "beta must not read alpha's snapshot");
        let mut vb = VaultStore::open(&b).unwrap();
        vb.unlock("pb").unwrap();
        assert!(!vb.list_connections().unwrap().iter().any(|c| c.label == "only-in-a"));
        let tree = git(None, &["--git-dir", remote.to_str().unwrap(), "ls-tree", "-r", "--name-only", "HEAD"]).unwrap();
        for f in ["alpha/oryxis-sync.bin", "alpha/oryxis-sync.sig", "beta/oryxis-sync.bin", "beta/oryxis-sync.sig"] {
            assert!(tree.lines().any(|l| l == f), "missing {f} in {tree}");
        }
        assert!(git_round(&url, &a, Some("pa".into()), &secret, "d", "../evil").is_err());
    }
}
