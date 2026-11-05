use super::*;
use crate::VaultStore;

fn store_in(dir: &Path) -> DeviceStore {
    DeviceStore::at(dir)
}

/// A vault file at `dir/vault.db` with no device store attached, so its
/// rows land in its own table (the shape every pre-registry install has).
fn legacy_vault(dir: &Path, rows: &[(&str, &str)]) -> PathBuf {
    let path = dir.join(FIRST_VAULT_FILE);
    let mut vault = VaultStore::open(&path).unwrap();
    vault.attach_device(None);
    for (k, v) in rows {
        vault.set_setting(k, v).unwrap();
    }
    path
}

#[test]
fn a_device_with_no_file_names_its_first_vault_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    assert_eq!(store.vaults(), vec![VaultEntry::first()]);
    assert_eq!(store.boot_vault().id, FIRST_VAULT_ID);
    assert_eq!(store.resolve(&store.boot_vault()), dir.path().join("vault.db"));
    assert_eq!(store.get("language"), None);
    assert!(!store.is_seeded());
    assert!(!store.path().exists(), "reads never create the file");
}

#[test]
fn a_setting_survives_a_new_store_on_the_same_folder() {
    let dir = tempfile::tempdir().unwrap();
    store_in(dir.path()).set("language", "pt-BR").unwrap();
    assert_eq!(store_in(dir.path()).get("language").as_deref(), Some("pt-BR"));
}

#[test]
fn a_read_sees_what_another_process_wrote_and_a_wipe_reads_as_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let mine = store_in(dir.path());
    let theirs = store_in(dir.path());
    mine.set("language", "de").unwrap();
    assert_eq!(theirs.get("language").as_deref(), Some("de"));
    // The other instance writes; the first one's cache must not answer.
    theirs.set("language", "fr").unwrap();
    assert_eq!(mine.get("language").as_deref(), Some("fr"));
    // The harness wipes the sandbox while the process lives on.
    std::fs::remove_file(mine.path()).unwrap();
    assert_eq!(mine.get("language"), None);
    assert_eq!(mine.vaults(), vec![VaultEntry::first()]);
}

#[test]
fn two_writers_keep_each_others_keys() {
    let dir = tempfile::tempdir().unwrap();
    let a = store_in(dir.path());
    let b = store_in(dir.path());
    // Both caches filled before either writes: a write that trusted its
    // cache would drop the other's key.
    assert_eq!(a.get("x"), None);
    assert_eq!(b.get("x"), None);
    a.set("language", "es").unwrap();
    b.set("renderer_backend", "opengl").unwrap();
    let fresh = store_in(dir.path());
    assert_eq!(fresh.get("language").as_deref(), Some("es"));
    assert_eq!(fresh.get("renderer_backend").as_deref(), Some("opengl"));
}

#[test]
fn fields_a_newer_build_wrote_survive_a_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(
        &path,
        r#"{"version": 9, "vaults": [{"id": "6f727978-6973-4000-8000-000000000001", "slug": "personal", "path": "vault.db", "color": "teal"}], "settings": {}, "future": [1, 2]}"#,
    )
    .unwrap();
    let store = store_in(dir.path());
    store.set("language", "ja").unwrap();
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written["future"], serde_json::json!([1, 2]));
    assert_eq!(written["vaults"][0]["color"], "teal");
    assert_eq!(written["version"], 9, "a newer format version is never lowered");
    assert_eq!(written["settings"]["language"], "ja");
}

#[test]
fn a_file_that_does_not_parse_reads_as_fresh_and_is_never_written_over() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(&path, b"{ not json").unwrap();
    let store = store_in(dir.path());
    assert_eq!(store.vaults(), vec![VaultEntry::first()]);
    // A write refuses: the registry a hand edit broke (every other
    // vault) must not be replaced by a fresh one-vault file.
    assert!(matches!(store.set("language", "it"), Err(DeviceError::Unparsable { .. })));
    assert_eq!(std::fs::read(&path).unwrap(), b"{ not json", "the file is kept as it is");
    assert_eq!(std::fs::read(dir.path().join("device.json.broken")).unwrap(), b"{ not json");
    assert_eq!(store_in(dir.path()).get("language"), None);
}

/// A transient unparsable `device.json` (another window or a sync
/// mid-write) must NOT collapse a warm registry to one vault: that is
/// what made the vault switcher vanish until the file's mtime changed
/// again. The cache keeps the last good registry on a failed re-read and
/// recovers once the write finishes.
#[test]
fn a_transient_unreadable_file_keeps_the_cached_registry() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    // Warm the cache with a second vault, so the registry is two.
    let work = store.create_entry("Work").unwrap();
    assert_eq!(store.vaults().len(), 2);

    // Simulate a mid-write: the file is briefly unparsable, and its
    // length changes so the cache's (mtime,len) stamp invalidates and a
    // re-read is attempted.
    let path = store.path();
    let good = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"{ half-written").unwrap();
    // The switcher reads this: it must still see BOTH vaults, not fold
    // back to one, even though the file on disk is garbage right now.
    assert_eq!(
        store.vaults().len(),
        2,
        "a transient bad read must not drop the registry to one vault"
    );
    assert!(store.entry(work.id).is_some(), "the second vault is still known");

    // The writer finishes (good bytes back, length changes again): the
    // next read picks the file up cleanly.
    std::fs::write(&path, &good).unwrap();
    assert_eq!(store.vaults().len(), 2);
}

/// A start names the file it could not read (issue #271: a portable
/// `data` folder on a USB key whose `device.json` was damaged): absent is
/// a first start, not a problem; a file that reads is none; one that does
/// not parse, or that cannot be read at all, is.
#[test]
fn a_start_is_told_when_the_file_cannot_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    assert!(store.read_problem().is_none(), "no file is a fresh device");
    store.set("language", "de").unwrap();
    assert!(store.read_problem().is_none(), "a file that reads");
    std::fs::write(store.path(), b"{ not json").unwrap();
    assert!(matches!(store.read_problem(), Some(DeviceError::Unparsable { .. })));
    std::fs::remove_file(store.path()).unwrap();
    // A folder where the file should be: reading it fails for a reason
    // other than absence.
    std::fs::create_dir(store.path()).unwrap();
    assert!(matches!(store.read_problem(), Some(DeviceError::Io { .. })));
}

/// A portable tree stores what lives on its own volume relative to
/// itself (issue #271): a vault opened from the USB key and a folder
/// setting there survive the key coming up as another drive. Reads hand
/// back absolute paths; a store of the profile's tree writes absolute.
#[test]
fn a_portable_tree_stores_its_own_volume_relative() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("Oryxis").join("data");
    std::fs::create_dir_all(&tree).unwrap();
    let vaults = root.path().join("Vaults");
    std::fs::create_dir_all(&vaults).unwrap();
    let work = vaults.join("work.db");
    std::fs::write(&work, b"").unwrap();

    let store = DeviceStore::at(&tree).with_relative_paths();
    let (entry, added) = store.register_existing(&work, "Work").unwrap();
    assert!(added);
    assert_eq!(entry.path, Path::new("..").join("..").join("Vaults").join("work.db"));
    assert_eq!(store.resolve(&entry), work, "resolved without the `..` steps");
    assert!(entry.is_outside(), "opened from elsewhere: removable from the list, no folder made for it");
    assert!(!store.register_existing(&work, "Again").unwrap().1, "the same file is not registered twice");

    store.set("snippet_output_dir", &vaults.display().to_string()).unwrap();
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(written["settings"]["snippet_output_dir"], Path::new("..").join("..").join("Vaults").display().to_string());
    assert_eq!(store.get("snippet_output_dir").as_deref(), Some(vaults.display().to_string().as_str()));
    assert!(store.settings().contains(&("snippet_output_dir".into(), vaults.display().to_string())));
    // Clearing stays empty, and a setting that is not a folder is never
    // touched.
    store.set("snippet_output_dir", "").unwrap();
    assert_eq!(store.get("snippet_output_dir").as_deref(), Some(""));
    store.set("language", "ja").unwrap();
    assert_eq!(store.get("language").as_deref(), Some("ja"));

    // The profile's tree: absolute, as before.
    let plain = DeviceStore::at(root.path().join("profile"));
    let (entry, _) = plain.register_existing(&work, "Work").unwrap();
    assert!(entry.path.is_absolute());
}

/// Only the files this install made are its own: the first vault and a
/// `vaults/<id>.db`. Anything else, absolute or relative, was opened from
/// where the user keeps it.
#[test]
fn only_the_vaults_this_install_made_are_inside() {
    let entry = |p: &str| VaultEntry { path: PathBuf::from(p), ..VaultEntry::first() };
    assert!(!entry("vault.db").is_outside());
    assert!(!entry("vaults/0f.db").is_outside());
    assert!(entry("../../Vaults/work.db").is_outside());
    assert!(entry("vaults/../x.db").is_outside());
    assert!(entry("copy.db").is_outside());
    assert!(entry(if cfg!(windows) { "V:\\work.db" } else { "/mnt/v/work.db" }).is_outside());
}

#[test]
fn a_registry_emptied_by_hand_gets_its_first_vault_back() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE_NAME), br#"{"vaults": []}"#).unwrap();
    assert_eq!(store_in(dir.path()).boot_vault(), VaultEntry::first());
}

#[test]
fn the_boot_vault_is_the_last_opened_while_it_is_registered() {
    let dir = tempfile::tempdir().unwrap();
    let other = Uuid::new_v4();
    let usb = tempfile::tempdir().unwrap();
    let work_file = usb.path().join("work.db");
    std::fs::write(&work_file, b"").unwrap();
    let json = serde_json::json!({
        "vaults": [
            {"id": FIRST_VAULT_ID, "slug": "personal", "path": "vault.db"},
            {"id": other, "slug": "work", "label": "Work", "path": work_file},
        ],
        "last_vault": other,
    });
    std::fs::write(dir.path().join(FILE_NAME), serde_json::to_vec(&json).unwrap()).unwrap();
    let store = store_in(dir.path());
    let boot = store.boot_vault();
    assert_eq!(boot.id, other);
    assert_eq!(store.resolve(&boot), work_file);
    // A last vault that is no longer registered falls back to the first.
    let json = serde_json::json!({
        "vaults": [{"id": FIRST_VAULT_ID, "slug": "personal", "path": "vault.db"}],
        "last_vault": other,
    });
    std::fs::write(dir.path().join(FILE_NAME), serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(store_in(dir.path()).boot_vault().id, FIRST_VAULT_ID);
}

#[test]
fn seeding_copies_only_the_device_keys_once() {
    let dir = tempfile::tempdir().unwrap();
    legacy_vault(
        dir.path(),
        &[("language", "pt-BR"), ("hotkey_new_tab", "ctrl+t"), ("app_theme", "Nord")],
    );
    let store = store_in(dir.path());
    assert_eq!(store.seed_from_first_vault().unwrap(), 2);
    assert!(store.is_seeded());
    assert_eq!(store.get("language").as_deref(), Some("pt-BR"));
    assert_eq!(store.get("hotkey_new_tab").as_deref(), Some("ctrl+t"));
    assert_eq!(store.get("app_theme"), None, "a vault setting stays in the vault");
    // Once: a later change in the vault's stale row is not copied again.
    store.set("language", "en").unwrap();
    assert_eq!(store.seed_from_first_vault().unwrap(), 0);
    assert_eq!(store.get("language").as_deref(), Some("en"));
}

#[test]
fn seeding_never_overwrites_a_key_set_before_it() {
    let dir = tempfile::tempdir().unwrap();
    legacy_vault(dir.path(), &[("language", "pt-BR"), ("debug_logging", "true")]);
    let store = store_in(dir.path());
    // Set on a start where the vault did not open (the error screen).
    store.set("language", "fr").unwrap();
    assert_eq!(store.seed_from_first_vault().unwrap(), 1);
    assert_eq!(store.get("language").as_deref(), Some("fr"));
    assert_eq!(store.get("debug_logging").as_deref(), Some("true"));
}

#[test]
fn a_fresh_install_is_seeded_with_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    assert_eq!(store.seed_from_first_vault().unwrap(), 0);
    assert!(store.is_seeded());
}

#[test]
fn a_vault_that_cannot_be_read_leaves_the_store_unseeded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FIRST_VAULT_FILE), vec![0x5a_u8; 8192]).unwrap();
    let store = store_in(dir.path());
    assert_eq!(store.seed_from_first_vault().unwrap(), 0);
    assert!(!store.is_seeded(), "the next start reads it again");
}

#[test]
fn seeding_reads_a_vault_another_handle_holds_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = legacy_vault(dir.path(), &[("language", "ko")]);
    let _held = VaultStore::open(&path).unwrap();
    let store = store_in(dir.path());
    assert_eq!(store.seed_from_first_vault().unwrap(), 1);
    assert_eq!(store.get("language").as_deref(), Some("ko"));
}

#[test]
fn a_vault_with_a_device_store_routes_device_keys_and_keeps_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let device = Arc::new(store_in(dir.path()));
    let path = dir.path().join("v.db");
    let mut vault = VaultStore::open(&path).unwrap();
    vault.attach_device(Some(device.clone()));

    vault.set_setting("language", "zh").unwrap();
    vault.set_setting("plugins_aws_pinned_version", "0.3.1").unwrap();
    vault.set_setting("app_theme", "Dracula").unwrap();
    assert_eq!(device.get("language").as_deref(), Some("zh"));
    assert_eq!(device.get("plugins_aws_pinned_version").as_deref(), Some("0.3.1"));
    assert_eq!(device.get("app_theme"), None);
    assert_eq!(vault.get_setting("language").unwrap().as_deref(), Some("zh"));
    assert_eq!(vault.get_setting("app_theme").unwrap().as_deref(), Some("Dracula"));

    // The vault's own table never saw the device keys.
    vault.attach_device(None);
    assert_eq!(vault.get_setting("language").unwrap(), None);
    assert_eq!(vault.get_setting("app_theme").unwrap().as_deref(), Some("Dracula"));
}

#[test]
fn listing_a_routed_vault_shows_the_device_values_not_its_stale_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = legacy_vault(dir.path(), &[("language", "pt-BR"), ("app_theme", "Nord")]);
    let device = Arc::new(store_in(dir.path()));
    device.set("language", "el").unwrap();
    let mut vault = VaultStore::open(&path).unwrap();
    vault.attach_device(Some(device));
    let listed = vault.list_settings().unwrap();
    let language: Vec<_> = listed.iter().filter(|(k, _)| k == "language").collect();
    assert_eq!(language, vec![&("language".to_owned(), "el".to_owned())]);
    assert!(listed.iter().any(|(k, v)| k == "app_theme" && v == "Nord"));
    assert!(listed.windows(2).all(|w| w[0].0 <= w[1].0), "sorted by key");
}

#[test]
fn deleting_by_prefix_reaches_the_device_store() {
    let dir = tempfile::tempdir().unwrap();
    let device = Arc::new(store_in(dir.path()));
    let mut vault = VaultStore::open(dir.path().join("v.db")).unwrap();
    vault.attach_device(Some(device.clone()));
    vault.set_setting("hotkey_close_tab", "ctrl+w").unwrap();
    vault.set_setting("hotkey_new_tab", "ctrl+t").unwrap();
    vault.set_setting("language", "th").unwrap();
    vault.delete_settings_with_prefix("hotkey_").unwrap();
    assert_eq!(device.get("hotkey_close_tab"), None);
    assert_eq!(device.get("hotkey_new_tab"), None);
    assert_eq!(device.get("language").as_deref(), Some("th"));
}

#[test]
fn no_device_key_is_a_sealed_setting() {
    // The device store is plaintext and readable with every vault
    // locked; a sealed row routed there would be ciphertext no vault key
    // re-seals on rotation, or worse, a secret written in the clear.
    for key in crate::store::ENCRYPTED_SETTING_KEYS {
        assert!(!is_device_key(key), "{key} is sealed and must stay in its vault");
    }
}

#[test]
fn the_first_vault_id_is_stable() {
    // Written into every device.json since the registry exists; a change
    // would orphan the first vault of every install.
    assert_eq!(FIRST_VAULT_ID.to_string(), "6f727978-6973-4000-8000-000000000001");
}

#[test]
fn a_slug_is_ascii_unique_and_never_empty() {
    assert_eq!(slug_for("Work", &[]), "work");
    assert_eq!(slug_for("  Clientes  ACME / 2026 ", &[]), "clientes-acme-2026");
    assert_eq!(slug_for("Ação Pessoal", &[]), "acao-pessoal");
    assert_eq!(slug_for("Straße", &[]), "strasse");
    assert_eq!(slug_for("工作", &[]), "vault");
    assert_eq!(slug_for("Work", &["work"]), "work-2");
    assert_eq!(slug_for("Work", &["work", "work-2"]), "work-3");
    assert_eq!(slug_for("", &["vault"]), "vault-2");
}

#[test]
fn a_created_vault_is_registered_under_the_vaults_folder() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let work = store.create_entry("Work").unwrap();
    assert_eq!(work.slug, "work");
    assert_eq!(work.label.as_deref(), Some("Work"));
    assert_eq!(store.resolve(&work), dir.path().join("vaults").join(format!("{}.db", work.id)));
    let fresh = store_in(dir.path());
    assert_eq!(fresh.vaults().len(), 2);
    assert_eq!(fresh.entry(work.id), Some(work.clone()));
    // A second "Work" gets its own slug.
    assert_eq!(store.create_entry("Work").unwrap().slug, "work-2");
}

#[test]
fn the_last_opened_vault_is_where_a_start_lands() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let work = store.create_entry("Work").unwrap();
    std::fs::create_dir_all(dir.path().join(VAULTS_DIR)).unwrap();
    std::fs::write(store.resolve(&work), b"").unwrap();
    store.set_last_vault(work.id).unwrap();
    assert_eq!(store_in(dir.path()).boot_vault().id, work.id);
}

#[test]
fn renaming_and_unregistering_keep_one_vault_at_least() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let work = store.create_entry("Work").unwrap();
    store.rename(work.id, "Clients").unwrap();
    assert_eq!(store.entry(work.id).unwrap().label.as_deref(), Some("Clients"));
    store.rename(work.id, "  ").unwrap();
    assert_eq!(store.entry(work.id).unwrap().label, None, "blank goes back to the default name");
    store.set_last_vault(work.id).unwrap();
    assert!(store.unregister(work.id).unwrap());
    assert_eq!(store.boot_vault().id, FIRST_VAULT_ID, "the last-opened pointer goes with it");
    assert!(!store.unregister(FIRST_VAULT_ID).unwrap(), "the only vault left stays");
    assert_eq!(store.vaults().len(), 1);
}

#[test]
fn an_existing_file_is_registered_once_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let file = elsewhere.path().join("usb.db");
    std::fs::write(&file, b"").unwrap();
    let store = store_in(dir.path());
    let (usb, new) = store.register_existing(&file, "USB").unwrap();
    assert!(new);
    assert!(usb.is_outside(), "kept where the user has it");
    assert_eq!(usb.slug, "usb");
    assert_eq!(std::fs::canonicalize(store.resolve(&usb)).unwrap(), std::fs::canonicalize(&file).unwrap());
    // The same file by another spelling is the same vault.
    let again = elsewhere.path().join(".").join("usb.db");
    let (same, new) = store.register_existing(&again, "Other").unwrap();
    assert!(!new);
    assert_eq!(same.id, usb.id);
    assert_eq!(store.vaults().len(), 2);
    // The install's own vaults are not outside.
    assert!(!VaultEntry::first().is_outside());
    assert!(!store.create_entry("Work").unwrap().is_outside());
}

#[test]
fn only_a_set_up_vault_passes_the_file_check_and_the_check_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.db");
    assert!(!is_vault_file(&missing));
    assert!(!missing.exists(), "the check never creates the file");
    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, b"not a database at all").unwrap();
    assert!(!is_vault_file(&junk));
    assert_eq!(std::fs::read(&junk).unwrap(), b"not a database at all");
    let path = dir.path().join("v.db");
    let mut vault = VaultStore::open(&path).unwrap();
    assert!(!is_vault_file(&path), "a vault with no master password yet holds nothing to open");
    vault.set_master_password("pw").unwrap();
    drop(vault);
    assert!(is_vault_file(&path));
}

#[test]
fn a_start_never_opens_a_vault_whose_file_went_away() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    // A fresh install: nothing on disk, the first vault is what opens.
    assert_eq!(store.boot_vault().id, FIRST_VAULT_ID);
    let usb = tempfile::tempdir().unwrap();
    let file = usb.path().join("usb.db");
    std::fs::write(&file, b"").unwrap();
    let (usb_vault, _) = store.register_existing(&file, "USB").unwrap();
    store.set_last_vault(usb_vault.id).unwrap();
    assert_eq!(store.boot_vault().id, usb_vault.id, "the last vault, while its file is there");
    std::fs::remove_file(&file).unwrap();
    // The key was pulled: the start goes to a vault that is there, here
    // the first one (still unmade: the first in the list).
    assert_eq!(store.boot_vault().id, FIRST_VAULT_ID);
    std::fs::write(dir.path().join(FIRST_VAULT_FILE), b"").unwrap();
    let work = store.create_entry("Work").unwrap();
    std::fs::create_dir_all(dir.path().join(VAULTS_DIR)).unwrap();
    std::fs::write(store.resolve(&work), b"").unwrap();
    std::fs::remove_file(dir.path().join(FIRST_VAULT_FILE)).unwrap();
    assert_eq!(store.boot_vault().id, work.id, "the first one whose file is there");
}

#[test]
fn a_vault_keeps_its_chord_and_loses_it_on_request() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let work = store.create_entry("Work").unwrap();
    assert_eq!(work.hotkey, None);
    store.set_hotkey(work.id, Some("ctrl+alt+2")).unwrap();
    assert_eq!(store_in(dir.path()).entry(work.id).unwrap().hotkey.as_deref(), Some("ctrl+alt+2"));
    store.set_hotkey(work.id, None).unwrap();
    assert_eq!(store.entry(work.id).unwrap().hotkey, None);
    // A file without the field (any build before it) reads as no chord.
    let raw = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
    assert!(!raw.contains("hotkey"), "an unset chord is not written");
}

#[test]
fn a_launch_naming_a_host_opens_the_vault_that_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let host = oryxis_core::models::Connection::new("db", "db.example.com");
    // The first vault (the last used) holds nothing; Work holds the host.
    drop(VaultStore::open(dir.path().join(FIRST_VAULT_FILE)).unwrap());
    let work = store.create_entry("Work").unwrap();
    std::fs::create_dir_all(dir.path().join(VAULTS_DIR)).unwrap();
    let mut work_db = VaultStore::open(store.resolve(&work)).unwrap();
    work_db.set_master_password("pw").unwrap();
    work_db.save_connection(&host, None).unwrap();
    work_db.lock();
    drop(work_db);
    store.set_last_vault(FIRST_VAULT_ID).unwrap();
    assert!(file_holds_connection(&store.resolve(&work), host.id), "read while locked");
    assert_eq!(store.boot_vault_for(Some(host.id)).id, work.id);
    // No host named, or one nobody holds: the usual start.
    assert_eq!(store.boot_vault_for(None).id, FIRST_VAULT_ID);
    assert_eq!(store.boot_vault_for(Some(uuid::Uuid::new_v4())).id, FIRST_VAULT_ID);
    // Both hold it (an import keeps ids): the last used wins.
    let first = VaultStore::open(dir.path().join(FIRST_VAULT_FILE)).unwrap();
    first.save_connection(&host, None).unwrap();
    drop(first);
    assert_eq!(store.boot_vault_for(Some(host.id)).id, FIRST_VAULT_ID);
}

#[test]
fn the_host_probe_writes_nothing_and_misses_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.db");
    assert!(!file_holds_connection(&missing, uuid::Uuid::new_v4()));
    assert!(!missing.exists(), "the probe never creates the file");
    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, b"not a database at all").unwrap();
    assert!(!file_holds_connection(&junk, uuid::Uuid::new_v4()));
    assert_eq!(std::fs::read(&junk).unwrap(), b"not a database at all");
}

#[test]
fn another_vaults_sync_target_reads_while_locked_and_never_a_sealed_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("work.db");
    let mut vault = VaultStore::open(&path).unwrap();
    vault.set_master_password("pw").unwrap();
    vault.set_setting("sync_folder_path", "/mnt/share/oryxis").unwrap();
    vault.set_setting("sync_webdav_password", "secret").unwrap();
    let host = oryxis_core::models::Connection::new("nas", "nas.lan");
    vault.save_connection(&host, None).unwrap();
    vault.lock();
    drop(vault);
    let rows = read_plain_settings(&path, &["sync_folder_path", "sync_webdav_password", "sync_enabled"]);
    assert_eq!(rows, vec![("sync_folder_path".to_string(), "/mnt/share/oryxis".to_string())]);
    assert_eq!(read_host_address(&path, host.id), Some(("nas.lan".to_string(), 22)));
    assert!(read_plain_settings(&dir.path().join("missing.db"), &["sync_folder_path"]).is_empty());
    assert!(!dir.path().join("missing.db").exists());
}

#[test]
fn the_first_vault_keeps_the_plain_mcp_variables_and_the_others_carry_their_slug() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let first = store.entry(FIRST_VAULT_ID).unwrap();
    assert_eq!(
        mcp_env_names(&first),
        McpEnvNames { token: "ORYXIS_MCP_TOKEN".into(), password: "ORYXIS_VAULT_PASSWORD".into() }
    );
    let work = store.create_entry("Área de Trabalho").unwrap();
    assert_eq!(mcp_env_names(&work).password, "ORYXIS_VAULT_PASSWORD_AREA_DE_TRABALHO");
    assert_eq!(mcp_env_names(&work).token, "ORYXIS_MCP_TOKEN_AREA_DE_TRABALHO");
}


#[test]
fn a_locked_vaults_kubernetes_accounts_read_without_its_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("work.db");
    let mut vault = VaultStore::open(&path).unwrap();
    vault.set_master_password("pw").unwrap();
    let mut k8s = oryxis_core::models::CloudProfile::new("cluster", "k8s");
    k8s.config = r#"{"kubeconfig":"/home/u/.oryxis/kubeconfig/a.yaml"}"#.into();
    vault.save_cloud_profile(&k8s, None).unwrap();
    vault.save_cloud_profile(&oryxis_core::models::CloudProfile::new("aws", "aws"), None).unwrap();
    vault.lock();
    drop(vault);
    assert_eq!(read_k8s_profile_configs(&path), Some(vec![k8s.config.clone()]), "only the Kubernetes ones");
    assert_eq!(read_k8s_profile_configs(&dir.path().join("gone.db")), None, "unknown, never empty");
}
