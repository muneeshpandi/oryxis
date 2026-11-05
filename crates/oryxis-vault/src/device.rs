//! The device store: `~/.oryxis/device.json`, what belongs to this
//! MACHINE rather than to any vault.
//!
//! Two things live here, both readable with every vault locked:
//!
//! - the vault REGISTRY (which vault files this device knows, the one
//!   the app opens first). Today it holds the one vault every install
//!   has, `vault.db`; multi-vault adds rows.
//! - the DEVICE SETTINGS: the few settings a process or a machine can
//!   only have one of (the UI language, the window geometry, the
//!   renderer, the folders and programs on this disk, the agent socket,
//!   the shortcut table). [`DEVICE_KEYS`] and [`DEVICE_PREFIXES`] are the
//!   one authority on which keys those are; every other setting belongs
//!   to a vault.
//!
//! **The routing is inside `VaultStore`.** `get_setting`, `set_setting`,
//! `list_settings` and `delete_settings_with_prefix` send a device key
//! here when the store has a device store attached, so the hundreds of
//! call sites that read and write settings by key keep doing so and
//! cannot pick the wrong side. `VaultStore::open` attaches the store
//! INSTALLED for the process ([`install`], called once by the app at
//! startup, before the first open); a process that installs none (the
//! MCP server, every test binary) keeps reading the vault's own rows.
//! Tests attach explicitly (`VaultStore::attach_device`) and NEVER
//! install: a test binary runs its tests in one process, and an
//! installed store would route every other test's settings into one
//! shared file.
//!
//! **Read-through.** Every read stats the file and re-reads it when it
//! changed (another process wrote it) or vanished (the harness wipes the
//! sandbox between tests while the process lives on); a missing file
//! reads as a fresh device, so nothing a previous run cached survives a
//! wipe. Writes are read-modify-write under an OS lock on
//! `device.json.lock`, temp + rename, so two processes (a second launch
//! on Linux / macOS) never lose each other's keys.
//!
//! **Migration.** The device keys used to be rows in the vault's own
//! `settings` table. [`DeviceStore::seed_from_first_vault`] copies them
//! ONCE, read-only, from the first vault, filling only keys the device
//! store does not hold yet, and marks the store seeded only after a read
//! that succeeded (a vault that does not open today, the rebuild's
//! patient, is read on the next start). The rows stay in the vault: a
//! downgraded build still finds them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// File name under the `.oryxis` folder.
pub const FILE_NAME: &str = "device.json";

/// The file name every install's first vault has, relative to the
/// `.oryxis` folder.
pub const FIRST_VAULT_FILE: &str = "vault.db";

/// The folder, inside `.oryxis`, every vault created after the first one
/// lives in (`vaults/<id>.db`).
pub const VAULTS_DIR: &str = "vaults";

/// The id of the vault that existed before the registry did (the
/// install's `vault.db`). Fixed, so a device store that has not been
/// written yet (first run, a wiped harness sandbox) names that vault
/// with the same id on every read.
pub const FIRST_VAULT_ID: Uuid = Uuid::from_u128(0x6f72_7978_6973_4000_8000_0000_0000_0001);

/// Format version written into the file. Unknown fields a newer build
/// adds are carried through untouched (`extra`), so an older build that
/// rewrites the file does not strip them.
const FORMAT_VERSION: u32 = 1;

/// Settings that belong to the device, by exact key. Kept beside
/// [`DEVICE_PREFIXES`]; `is_device_key` reads both.
pub const DEVICE_KEYS: &[&str] = &[
    // `i18n::t` and the RTL switch are process-wide: two windows cannot
    // speak two languages.
    "language",
    "layout_direction",
    // One remembered geometry, one tray.
    "window_width",
    "window_height",
    "window_pos_x",
    "window_pos_y",
    "window_maximized",
    "window_fullscreen",
    "window_fullscreen_immersive",
    "close_to_tray",
    "minimize_to_tray",
    // One renderer, one log per process.
    "renderer_backend",
    "perf_overlay",
    "debug_logging",
    // One binary, one plugin cache, one network posture.
    "offline_mode",
    "download_mirror",
    "auto_check_updates",
    "update_channel",
    "skipped_update_version",
    // Programs and folders on THIS disk.
    "local_terminals",
    "local_terminal_default",
    "sftp_default_editor",
    "zmodem_download_dir",
    "session_log_file_dir",
    "command_history_file_dir",
    "snippet_output_dir",
    // One agent socket / pipe.
    "agent_server_enabled",
    "agent_server_confirm",
    "agent_server_allow_add",
    "agent_server_openssh_pipe",
    // One sync listener; the machine's name.
    "sync_listen_port",
    "sync_listen_port_auto",
    "sync_device_name",
    // How many vaults this device has decides whether the switcher
    // shows; hiding it for a single vault is the device's choice.
    "vault_switcher_hide_single",
    "vault_switcher_place",
    "vault_start_picker",
    // Binding state of the shortcut table (the table itself is the
    // `hotkey_` prefix).
    "middle_click_paste",
    "middle_click_paste_migrated",
    "wheel_zoom_migrated",
];

/// Device settings that name a FOLDER. In a portable tree a folder on
/// the tree's own volume is stored relative to the tree
/// ([`DeviceStore::with_relative_paths`]), so the setting survives the
/// USB key coming up as another drive; reads hand back the absolute
/// path either way. The program settings (`local_terminals`,
/// `sftp_default_editor`) are command lines, not paths, and stay as
/// written.
pub const DEVICE_FOLDER_KEYS: &[&str] = &[
    "zmodem_download_dir",
    "session_log_file_dir",
    "command_history_file_dir",
    "snippet_output_dir",
];

/// Settings that belong to the device, by key prefix: the shortcut
/// table (`hotkey_<action>`) and the plugin cache's per-plugin state
/// (`plugins_<id>_pinned_version`, `plugins_<id>_auto_update`,
/// `plugins_auto_update_global`).
pub const DEVICE_PREFIXES: &[&str] = &["hotkey_", "plugins_"];

/// Whether `key` is a device setting (lives in `device.json`) rather
/// than a vault setting.
pub fn is_device_key(key: &str) -> bool {
    DEVICE_KEYS.contains(&key) || DEVICE_PREFIXES.iter().any(|p| key.starts_with(p))
}

/// One vault this device knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultEntry {
    pub id: Uuid,
    /// Unique, ASCII: what `--vault` and the per-vault MCP variables
    /// name.
    pub slug: String,
    /// What the user called it. `None` = the app's translated default
    /// name (the first vault of every install has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The file. Relative = inside the `.oryxis` folder (so a moved
    /// `ORYXIS_HOME` or a portable `data` folder takes its vaults along);
    /// absolute = where the user put it.
    pub path: PathBuf,
    /// The chord that moves the window in use to this vault, in the
    /// hotkey table's text form (`"ctrl+alt+1"`); `None` = none. Lives on
    /// the entry so it goes with the vault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl VaultEntry {
    /// Whether the file was opened from where the user keeps it rather
    /// than made by this install (the first `vault.db`, a `vaults/<id>.db`):
    /// such a vault may be taken off the list with its file left alone,
    /// and no folder is ever made for it. An absolute entry, and a
    /// relative one a portable tree stores for a file elsewhere on its
    /// volume (`../../Vaults/work.db`).
    pub fn is_outside(&self) -> bool {
        use std::path::Component;
        let made_here = match self.path.components().collect::<Vec<_>>().as_slice() {
            [Component::Normal(name)] => *name == FIRST_VAULT_FILE,
            [Component::Normal(dir), Component::Normal(_)] => *dir == VAULTS_DIR,
            _ => false,
        };
        !made_here
    }

    /// The entry for the install's `vault.db`.
    pub fn first() -> Self {
        Self {
            id: FIRST_VAULT_ID,
            slug: "personal".to_owned(),
            label: None,
            path: PathBuf::from(FIRST_VAULT_FILE),
            hotkey: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// The file's content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DeviceFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    vaults: Vec<VaultEntry>,
    /// The vault the app opened last; the one a start opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_vault: Option<Uuid>,
    #[serde(default)]
    settings: BTreeMap<String, String>,
    /// The device keys were copied from the first vault (or there was
    /// nothing to copy). See [`DeviceStore::seed_from_first_vault`].
    #[serde(default)]
    seeded: bool,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

impl DeviceFile {
    /// A device that has never written its file: the first vault and
    /// nothing else.
    fn fresh() -> Self {
        Self {
            version: FORMAT_VERSION,
            vaults: vec![VaultEntry::first()],
            last_vault: None,
            settings: BTreeMap::new(),
            seeded: false,
            extra: serde_json::Map::new(),
        }
    }

    /// Repairs what a hand edit can break: a registry with no vault at
    /// all gets the first one back (the app always has a vault to open).
    fn normalized(mut self) -> Self {
        if self.vaults.is_empty() {
            self.vaults.push(VaultEntry::first());
        }
        self
    }
}

/// What was on disk when the cache was filled: a changed stamp means
/// another process (or a wipe) changed the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stamp {
    Missing,
    Present { modified: Option<std::time::SystemTime>, len: u64 },
}

fn stamp_of(path: &Path) -> Stamp {
    match std::fs::metadata(path) {
        Ok(meta) => Stamp::Present { modified: meta.modified().ok(), len: meta.len() },
        Err(_) => Stamp::Missing,
    }
}

struct Cached {
    stamp: Option<Stamp>,
    file: DeviceFile,
}

/// Errors of the device store. Reads never fail (an unreadable file is
/// reported once and read as fresh); writes do.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("device store I/O at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("device store encode: {0}")]
    Encode(#[from] serde_json::Error),
    /// The file is there and does not parse: nothing is written over it
    /// (the registry it holds would go with it), reads see a fresh store.
    #[error("device store {path} does not parse ({reason}); not writing over it")]
    Unparsable { path: PathBuf, reason: String },
}

/// `~/.oryxis/device.json`, read through, written under a lock.
pub struct DeviceStore {
    dir: PathBuf,
    /// Store paths on the tree's own volume relative to it (a portable
    /// tree, [`Self::with_relative_paths`]).
    relative_paths: bool,
    cache: Mutex<Cached>,
}

impl std::fmt::Debug for DeviceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceStore").field("dir", &self.dir).finish_non_exhaustive()
    }
}

impl DeviceStore {
    /// The store whose file is `<dir>/device.json`, `dir` being the
    /// `.oryxis` folder. Nothing is read or created yet.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            relative_paths: false,
            cache: Mutex::new(Cached { stamp: None, file: DeviceFile::fresh() }),
        }
    }

    /// The store of a portable tree: a vault registered from the tree's
    /// own volume and a folder setting on it are written relative to the
    /// tree (`oryxis_core::paths::relative_on_same_volume`), so the USB
    /// key keeps working under another drive letter or mount point.
    /// Another volume (a VeraCrypt drive) keeps its absolute path.
    pub fn with_relative_paths(mut self) -> Self {
        self.relative_paths = true;
        self
    }

    /// `path` as this store writes it: relative to the tree when the
    /// store keeps relative paths and `path` is on the tree's volume,
    /// absolute otherwise.
    fn stored_path(&self, path: &Path) -> PathBuf {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if self.relative_paths
            && let Some(relative) = oryxis_core::paths::relative_on_same_volume(&self.dir, &absolute)
        {
            return relative;
        }
        absolute
    }

    /// A stored folder setting as the app uses it: a relative value is
    /// the tree's own, resolved against it; anything else as written.
    fn read_folder(&self, key: &str, value: String) -> String {
        if !DEVICE_FOLDER_KEYS.contains(&key) || value.is_empty() || Path::new(&value).is_absolute() {
            return value;
        }
        oryxis_core::paths::join_lexically(&self.dir, Path::new(&value)).display().to_string()
    }

    /// The `.oryxis` folder this store lives in (relative vault paths
    /// resolve against it).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Why the file on disk cannot be read as it is: it does not parse,
    /// or reading it failed for another reason than its absence (a
    /// medium that went away, a permission). Reads then see a fresh
    /// device and every write is refused, so a start shows this rather
    /// than leaving the user to wonder where their vaults went (a
    /// portable `data` folder on a USB key, issue #271). `None` when the
    /// file reads or is not there (a first start).
    pub fn read_problem(&self) -> Option<DeviceError> {
        parse_file(&self.path()).err()
    }

    /// The file itself.
    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    fn lock_path(&self) -> PathBuf {
        self.dir.join(format!("{FILE_NAME}.lock"))
    }

    /// The cache, refreshed from disk when the file changed or vanished.
    /// A poisoned mutex (a panic while it was held: the panic hook writes
    /// the renderer fallback through here) is taken over, not
    /// propagated: the content is a plain map, never half-written in
    /// memory.
    fn with_current<R>(&self, f: impl FnOnce(&DeviceFile) -> R) -> R {
        let path = self.path();
        let mut cached = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let now = stamp_of(&path);
        if cached.stamp != Some(now) {
            // A transient read failure must NOT collapse a good cache to
            // `fresh()`. When another window (or a sync) is mid-write,
            // `device.json` can momentarily be half-written / unparsable
            // or briefly unreadable; `read_file` would turn that into a
            // one-vault `fresh()` registry, which made the vault switcher
            // vanish until the file's mtime changed again (it won't, the
            // bad result just got cached under the new stamp). So:
            //   - the file genuinely missing (NotFound) → legitimately
            //     fresh (first run / removed), accept it and stamp it.
            //   - any OTHER error (unparsable, I/O) while we already hold
            //     a registry → keep the cached one and DON'T advance the
            //     stamp, so the next access retries once the writer is
            //     done. With no prior cache, fall back to fresh so a true
            //     first read still works.
            match parse_file(&path) {
                Ok(file) => {
                    cached.file = file;
                    cached.stamp = Some(now);
                }
                Err(_) if cached.stamp.is_some() => {
                    // Keep the last good registry; leave the stamp as-is
                    // so we re-read on the next call (the writer finishes
                    // in milliseconds).
                }
                Err(_) => {
                    cached.file = DeviceFile::fresh();
                    cached.stamp = Some(now);
                }
            }
        }
        f(&cached.file)
    }

    /// Read-modify-write: the file is re-read under the OS lock (another
    /// process may have written since the cache was filled), `change`
    /// applied, the result written temp + rename and kept as the cache.
    fn update(&self, change: impl FnOnce(&mut DeviceFile)) -> Result<(), DeviceError> {
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| DeviceError::Io { path, source }
        };
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        let lock_path = self.lock_path();
        let mut lock_options = std::fs::OpenOptions::new();
        lock_options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            lock_options.mode(0o600);
        }
        let lock = lock_options.open(&lock_path).map_err(io(&lock_path))?;
        // A file system with no locks (some network mounts): the
        // in-process mutex below still serializes this process.
        if let Err(e) = lock.lock() {
            tracing::warn!(error = %e, "no file lock for the device store; writing without one");
        }
        let path = self.path();
        let mut cached = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        // Never over a file that is there and does not parse.
        let mut file = parse_file(&path)?;
        change(&mut file);
        file.version = file.version.max(FORMAT_VERSION);
        write_file(&path, &file)?;
        cached.stamp = Some(stamp_of(&path));
        cached.file = file;
        drop(cached);
        drop(lock);
        Ok(())
    }

    // -- settings ---------------------------------------------------------

    /// A device setting, `None` when never set.
    pub fn get(&self, key: &str) -> Option<String> {
        self.with_current(|f| f.settings.get(key).cloned()).map(|v| self.read_folder(key, v))
    }

    /// Store a device setting. A folder setting ([`DEVICE_FOLDER_KEYS`])
    /// is written the way [`Self::with_relative_paths`] says.
    pub fn set(&self, key: &str, value: &str) -> Result<(), DeviceError> {
        let value = if DEVICE_FOLDER_KEYS.contains(&key) && !value.is_empty() {
            self.stored_path(Path::new(value)).display().to_string()
        } else {
            value.to_owned()
        };
        self.update(|f| {
            f.settings.insert(key.to_owned(), value);
        })
    }

    /// Every device setting, sorted by key, folders resolved like [`Self::get`].
    pub fn settings(&self) -> Vec<(String, String)> {
        self.with_current(|f| f.settings.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>())
            .into_iter()
            .map(|(k, v)| {
                let v = self.read_folder(&k, v);
                (k, v)
            })
            .collect()
    }

    /// Remove every device setting whose key starts with `prefix`.
    pub fn delete_with_prefix(&self, prefix: &str) -> Result<(), DeviceError> {
        if !self.with_current(|f| f.settings.keys().any(|k| k.starts_with(prefix))) {
            return Ok(());
        }
        self.update(|f| f.settings.retain(|k, _| !k.starts_with(prefix)))
    }

    // -- registry ---------------------------------------------------------

    /// Every vault this device knows, in registry order. Never empty.
    pub fn vaults(&self) -> Vec<VaultEntry> {
        self.with_current(|f| f.vaults.clone())
    }

    /// The vault a start opens: the last one opened while it is still
    /// registered and its file is there, else the first in the list whose
    /// file is there, else the first in the list (a fresh install: its
    /// first vault is made by this open). A vault whose file went away (a
    /// USB key) is never opened at a start: the open would make an empty
    /// vault in its place.
    pub fn boot_vault(&self) -> VaultEntry {
        let present = |v: &VaultEntry| self.resolve(v).is_file();
        self.with_current(|f| {
            f.last_vault
                .and_then(|id| f.vaults.iter().find(|v| v.id == id))
                .filter(|v| present(v))
                .or_else(|| f.vaults.iter().find(|v| present(v)))
                .or_else(|| f.vaults.first())
                .cloned()
                .unwrap_or_else(VaultEntry::first)
        })
    }

    /// The vault a start opens: when the launch names a saved host
    /// (`--connect <id>`, a JumpList entry), the registered vault that
    /// holds it, read while locked (`connections.id` is plaintext), the
    /// [`Self::boot_vault`] first when it is one of them (an imported
    /// host keeps its id, so two vaults can hold it); else
    /// [`Self::boot_vault`].
    pub fn boot_vault_for(&self, connect: Option<Uuid>) -> VaultEntry {
        let usual = self.boot_vault();
        let Some(id) = connect else {
            return usual;
        };
        if file_holds_connection(&self.resolve(&usual), id) {
            return usual;
        }
        let entries = self.with_current(|f| f.vaults.clone());
        entries
            .into_iter()
            .find(|v| file_holds_connection(&self.resolve(v), id))
            .unwrap_or(usual)
    }

    /// Where `entry`'s file is: a relative path inside this store's
    /// folder, an absolute one as written.
    pub fn resolve(&self, entry: &VaultEntry) -> PathBuf {
        if entry.path.is_absolute() {
            entry.path.clone()
        } else {
            oryxis_core::paths::join_lexically(&self.dir, &entry.path)
        }
    }

    /// The registered vault `id`.
    pub fn entry(&self, id: Uuid) -> Option<VaultEntry> {
        self.with_current(|f| f.vaults.iter().find(|v| v.id == id).cloned())
    }

    /// Register a new vault called `label`: a fresh id, a slug no other
    /// vault has, the file `vaults/<id>.db` inside this store's folder
    /// (created by the first open). Appended to the registry.
    pub fn create_entry(&self, label: &str) -> Result<VaultEntry, DeviceError> {
        let mut created = None;
        self.update(|f| {
            let id = Uuid::new_v4();
            let taken: Vec<&str> = f.vaults.iter().map(|v| v.slug.as_str()).collect();
            let entry = VaultEntry {
                id,
                slug: slug_for(label, &taken),
                label: Some(label.trim().to_owned()).filter(|l| !l.is_empty()),
                path: PathBuf::from(VAULTS_DIR).join(format!("{id}.db")),
                hotkey: None,
                extra: serde_json::Map::new(),
            };
            f.vaults.push(entry.clone());
            created = Some(entry);
        })?;
        Ok(created.expect("set by the update"))
    }

    /// Register the vault file at `path` (where the user keeps it), called
    /// `label`. Stored absolute, as given, or relative to a portable tree
    /// on its own volume ([`Self::with_relative_paths`]): the canonical
    /// form is only how two spellings of one file are told apart (on
    /// Windows it is a `\\?\` path no one should see). A file already
    /// registered is not registered twice: answers that entry and `false`.
    pub fn register_existing(&self, path: &Path, label: &str) -> Result<(VaultEntry, bool), DeviceError> {
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let wanted = canonical(path);
        let mut out = None;
        self.update(|f| {
            if let Some(known) = f.vaults.iter().find(|v| canonical(&self.resolve(v)) == wanted) {
                out = Some((known.clone(), false));
                return;
            }
            let taken: Vec<&str> = f.vaults.iter().map(|v| v.slug.as_str()).collect();
            let entry = VaultEntry {
                id: Uuid::new_v4(),
                slug: slug_for(label, &taken),
                label: Some(label.trim().to_owned()).filter(|l| !l.is_empty()),
                path: self.stored_path(path),
                hotkey: None,
                extra: serde_json::Map::new(),
            };
            f.vaults.push(entry.clone());
            out = Some((entry, true));
        })?;
        Ok(out.expect("set by the update"))
    }

    /// Remember `id` as the vault a start opens.
    pub fn set_last_vault(&self, id: Uuid) -> Result<(), DeviceError> {
        if self.with_current(|f| f.last_vault == Some(id)) {
            return Ok(());
        }
        self.update(|f| f.last_vault = Some(id))
    }

    /// Rename vault `id`. An empty label goes back to the default name.
    pub fn rename(&self, id: Uuid, label: &str) -> Result<(), DeviceError> {
        self.update(|f| {
            if let Some(v) = f.vaults.iter_mut().find(|v| v.id == id) {
                v.label = Some(label.trim().to_owned()).filter(|l| !l.is_empty());
            }
        })
    }

    /// Set (or, with `None`, clear) the chord of vault `id`.
    pub fn set_hotkey(&self, id: Uuid, hotkey: Option<&str>) -> Result<(), DeviceError> {
        self.update(|f| {
            if let Some(v) = f.vaults.iter_mut().find(|v| v.id == id) {
                v.hotkey = hotkey.map(str::to_owned);
            }
        })
    }

    /// Take vault `id` off the registry; its file is left where it is.
    /// The last vault cannot go (a device always has one to open):
    /// answers `false` then, and for an id that is not registered.
    pub fn unregister(&self, id: Uuid) -> Result<bool, DeviceError> {
        let mut removed = false;
        self.update(|f| {
            if f.vaults.len() > 1 && f.vaults.iter().any(|v| v.id == id) {
                f.vaults.retain(|v| v.id != id);
                if f.last_vault == Some(id) {
                    f.last_vault = None;
                }
                removed = true;
            }
        })?;
        Ok(removed)
    }

    // -- migration --------------------------------------------------------

    /// Whether the device keys were already copied out of the first
    /// vault.
    pub fn is_seeded(&self) -> bool {
        self.with_current(|f| f.seeded)
    }

    /// Copy the device keys out of the first vault's `settings` table,
    /// once. Read-only (no schema step, no write lock: the vault may be
    /// damaged or held by another process), only keys the store does not
    /// hold yet (one set meanwhile, on a start where the vault did not
    /// open, wins). Marks the store seeded when the read succeeded or
    /// there was no vault to read (a fresh install); a read that FAILED
    /// leaves it unseeded, so the next start tries again.
    ///
    /// Returns how many keys were copied.
    pub fn seed_from_first_vault(&self) -> Result<usize, DeviceError> {
        if self.is_seeded() {
            return Ok(0);
        }
        let first = self.with_current(|f| {
            f.vaults.iter().find(|v| v.id == FIRST_VAULT_ID).cloned()
        });
        let rows = match first.map(|e| self.resolve(&e)) {
            Some(path) if path.exists() => match read_device_rows(&path) {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "device settings not copied from the vault yet");
                    return Ok(0);
                }
            },
            // No first vault on disk (fresh install, or registered away):
            // nothing to copy, ever.
            _ => Vec::new(),
        };
        let mut copied = 0;
        self.update(|f| {
            for (key, value) in rows {
                if let std::collections::btree_map::Entry::Vacant(slot) = f.settings.entry(key) {
                    slot.insert(value);
                    copied += 1;
                }
            }
            f.seeded = true;
        })?;
        Ok(copied)
    }
}

/// A slug for `label` none of `taken` has: lowercase ASCII letters and
/// digits (Latin accents folded: "Ação" is `acao`), runs of anything
/// else as one `-`, trimmed; `vault` when nothing is left; `-2`, `-3`..
/// until it is free. What `--vault` and the per-vault MCP variables
/// name.
pub fn slug_for(label: &str, taken: &[&str]) -> String {
    let mut base = String::new();
    for c in label.trim().chars().flat_map(char::to_lowercase) {
        let folded = fold_latin(c);
        if !folded.is_empty() {
            base.push_str(folded);
        } else if c.is_ascii_alphanumeric() {
            base.push(c);
        } else if !base.ends_with('-') && !base.is_empty() {
            base.push('-');
        }
    }
    let base = base.trim_end_matches('-').to_owned();
    let base = if base.is_empty() { "vault".to_owned() } else { base };
    if !taken.contains(&base.as_str()) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|s| !taken.contains(&s.as_str()))
        .expect("an unbounded range finds a free suffix")
}

/// The ASCII letters a lowercase accented Latin letter stands for, empty
/// for anything else (ASCII included).
fn fold_latin(c: char) -> &'static str {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'ğ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
        'ł' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
        'œ' => "oe",
        'ř' => "r",
        'ś' | 'š' | 'ş' => "s",
        'ß' => "ss",
        'ť' | 'ţ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'þ' => "th",
        _ => "",
    }
}

/// The file's content as a `Result`: `Ok(fresh)` when the file is
/// genuinely ABSENT (first run / removed), `Ok(parsed)` when it reads,
/// and `Err` when it exists but is unparsable or briefly unreadable (a
/// WRITE refuses it, and `with_current` keeps the last good cache rather
/// than collapsing the registry — every other vault would otherwise
/// vanish with the file). A file that does not parse has a copy kept
/// beside it ([`keep_broken_copy`]).
fn parse_file(path: &Path) -> Result<DeviceFile, DeviceError> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<DeviceFile>(&bytes) {
            Ok(file) => Ok(file.normalized()),
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "device store does not parse; reading it as fresh");
                keep_broken_copy(path);
                Err(DeviceError::Unparsable { path: path.to_path_buf(), reason: e.to_string() })
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DeviceFile::fresh()),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "device store unreadable; reading it as fresh");
            Err(DeviceError::Io { path: path.to_path_buf(), source: e })
        }
    }
}

/// A file that did not parse is copied to `device.json.broken` once, so
/// a hand edit's result is kept whatever happens to the file next.
fn keep_broken_copy(path: &Path) {
    let broken = path.with_extension("json.broken");
    if !broken.exists() {
        let _ = std::fs::copy(path, &broken);
    }
}

fn write_file(path: &Path, file: &DeviceFile) -> Result<(), DeviceError> {
    let mut bytes = serde_json::to_vec_pretty(file)?;
    bytes.push(b'\n');
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let io = |p: &Path| {
        let path = p.to_path_buf();
        move |source| DeviceError::Io { path, source }
    };
    write_private(&tmp, &bytes).map_err(io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        DeviceError::Io { path: path.to_path_buf(), source: e }
    })
}

/// Owner-only on unix, like the vault: the file names where every vault
/// lives.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut out = options.open(path)?;
    out.write_all(bytes)?;
    out.sync_all()
}

/// Whether `path` is an Oryxis vault with a master password set, read
/// without writing anything (no schema step: a file that is something
/// else must come out of the check unchanged).
pub fn is_vault_file(path: &Path) -> bool {
    let probe = || -> rusqlite::Result<bool> {
        let db = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM vault_meta WHERE key = 'password_check')",
            [],
            |row| row.get(0),
        )
    };
    path.is_file() && probe().unwrap_or(false)
}

/// Whether the vault file at `path` holds saved host `id`, read-only,
/// without its password (the id column is plaintext). A missing or
/// unreadable file holds nothing.
pub fn file_holds_connection(path: &Path, id: Uuid) -> bool {
    let probe = || -> rusqlite::Result<bool> {
        let db = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM connections WHERE id = ?1)",
            [id.to_string()],
            |row| row.get(0),
        )
    };
    path.is_file() && probe().unwrap_or(false)
}

/// A vault file's plaintext `settings` rows among `keys`, read-only and
/// without its password (only `ENCRYPTED_SETTING_KEYS` are sealed): what
/// another vault, locked or not open, is configured to do. A missing or
/// unreadable file has no rows.
pub fn read_plain_settings(path: &Path, keys: &[&str]) -> Vec<(String, String)> {
    let probe = || -> rusqlite::Result<Vec<(String, String)>> {
        let db = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut stmt = db.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (key, value) = row?;
            if keys.contains(&key.as_str()) && !crate::store::ENCRYPTED_SETTING_KEYS.contains(&key.as_str()) {
                out.push((key, value));
            }
        }
        Ok(out)
    };
    if !path.is_file() {
        return Vec::new();
    }
    probe().unwrap_or_default()
}

/// A saved host's address (hostname, port) in a vault file, read-only
/// and without its password (both columns are plaintext).
pub fn read_host_address(path: &Path, id: Uuid) -> Option<(String, u16)> {
    let probe = || -> rusqlite::Result<(String, u16)> {
        let db = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.query_row(
            "SELECT hostname, port FROM connections WHERE id = ?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    };
    if !path.is_file() {
        return None;
    }
    probe().ok()
}

/// Whether the vault file at `path` has an ACTIVE sync peer `peer_id`,
/// read-only and without its password (the id column is plaintext): a
/// remote identity pairs with one vault per device.
pub fn file_holds_sync_peer(path: &Path, peer_id: Uuid) -> bool {
    let probe = || -> rusqlite::Result<bool> {
        let db = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_peers WHERE peer_id = ?1 AND is_active = 1)",
            [peer_id.to_string()],
            |row| row.get(0),
        )
    };
    path.is_file() && probe().unwrap_or(false)
}

/// The `config` of every Kubernetes cloud account in the vault file at
/// `path`, read-only and without its password (the column is plaintext;
/// secrets sit in `secret`). `None` when the file is not there or cannot
/// be read: the caller must then treat its references as unknown.
pub fn read_k8s_profile_configs(path: &Path) -> Option<Vec<String>> {
    let probe = || -> rusqlite::Result<Vec<String>> {
        let db = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut stmt = db.prepare("SELECT config FROM cloud_profiles WHERE provider = 'k8s'")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    };
    if !path.is_file() {
        return None;
    }
    probe().ok()
}

/// The device keys of a vault file's `settings` table, read-only.
fn read_device_rows(path: &Path) -> rusqlite::Result<Vec<(String, String)>> {
    let db = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let mut stmt = db.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (key, value) = row?;
        if is_device_key(&key) {
            out.push((key, value));
        }
    }
    Ok(out)
}

// -- the process's store ----------------------------------------------------

static INSTALLED: OnceLock<Arc<DeviceStore>> = OnceLock::new();

/// Install the device store for this process: every `VaultStore` opened
/// from now on routes device keys to it. Once per process (the app's
/// `main`, before the first vault open); a second call is ignored and
/// answers `false`. Tests never call it (module docs).
pub fn install(store: Arc<DeviceStore>) -> bool {
    INSTALLED.set(store).is_ok()
}

/// The store installed for this process, if any.
pub fn installed() -> Option<Arc<DeviceStore>> {
    INSTALLED.get().cloned()
}

/// The store for the app's own `.oryxis` folder (every override of
/// `oryxis_core::paths` included: `ORYXIS_DATA_DIR`, `ORYXIS_HOME`, the
/// portable `data` folder), `None` when nothing resolves.
pub fn for_app_dir() -> Option<DeviceStore> {
    let tree = oryxis_core::paths::data_dir()?;
    let store = DeviceStore::at(tree.path);
    Some(if tree.source == oryxis_core::paths::DataDirSource::Portable { store.with_relative_paths() } else { store })
}

/// The environment variables a vault's MCP secrets ride in, for the
/// app writing a client config and `oryxis-mcp` reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpEnvNames {
    /// The MCP token (`mcp_server_token`).
    pub token: String,
    /// The master password, when the user embeds it.
    pub password: String,
}

/// [`McpEnvNames`] of `entry`: `ORYXIS_MCP_TOKEN_<SLUG>` and
/// `ORYXIS_VAULT_PASSWORD_<SLUG>`, the slug upper-cased with `-` as `_`.
/// The FIRST vault keeps the plain `ORYXIS_MCP_TOKEN` /
/// `ORYXIS_VAULT_PASSWORD`, so a config written before there were
/// several vaults (and an `oryxis-mcp` that knows only those) keeps
/// serving it.
pub fn mcp_env_names(entry: &VaultEntry) -> McpEnvNames {
    if entry.id == FIRST_VAULT_ID {
        return McpEnvNames { token: "ORYXIS_MCP_TOKEN".into(), password: "ORYXIS_VAULT_PASSWORD".into() };
    }
    let suffix = entry.slug.to_ascii_uppercase().replace('-', "_");
    McpEnvNames {
        token: format!("ORYXIS_MCP_TOKEN_{suffix}"),
        password: format!("ORYXIS_VAULT_PASSWORD_{suffix}"),
    }
}

/// The file the app opens at start: the installed store's boot vault,
/// else (no store installed: the MCP server) the install's `vault.db`.
pub fn boot_vault_path() -> Option<PathBuf> {
    match installed() {
        Some(store) => Some(store.resolve(&store.boot_vault())),
        None => crate::VaultStore::default_path(),
    }
}

#[cfg(test)]
mod tests;
