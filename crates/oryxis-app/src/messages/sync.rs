//! P2P + SFTP-snapshot sync messages, wrapped by
//! [`crate::messages::Message::Sync`]. Handled by `Oryxis::handle_sync`
//! (the SFTP-snapshot round logic lives in `dispatch_sftp_sync`).

#[derive(Debug, Clone)]
pub enum SyncMessage {
    /// A folder-sync round finished: `Ok(records pulled)` or the
    /// failure to show inline. The transport is local file IO, so the
    /// only failures are a bad path, a wrong passphrase, or a snapshot
    /// that will not decrypt.
    FolderRoundFinished(Result<usize, String>),
    /// Settings > Sync: the folder transport's path.
    FolderPathChanged(String),
    /// "Sync now" for the folder transport.
    FolderSyncNow,
    /// Open the OS folder picker for the snapshot directory.
    FolderPickDirectory,
    /// The picker returned; `None` is a cancel and stays silent.
    FolderDirectoryPicked(Option<String>),
    /// A WebDAV-sync round finished: `Ok(records pulled)` or the
    /// failure to show inline.
    WebdavRoundFinished(Result<usize, String>),
    /// Settings > Sync: the WebDAV transport's collection URL and the
    /// account it authenticates as.
    WebdavUrlChanged(String),
    WebdavUserChanged(String),
    /// The account's password on that server, `Redacted` like every
    /// other secret-bearing variant
    /// (`secret_bearing_variants_carry_redacted`). A DIFFERENT secret
    /// from the group passphrase.
    WebdavPasswordChanged(super::Redacted),
    /// "Sync now" for the WebDAV transport.
    WebdavSyncNow,
    /// A Git-sync round finished: `Ok(records pulled)` or the failure.
    GitRoundFinished(Result<usize, String>),
    /// Settings > Sync: the Git transport's remote URL.
    GitRemoteChanged(String),
    GitFolderChanged(String),
    /// "Sync now" for the Git transport.
    GitSyncNow,
    /// Result of the off-thread `git --version` probe, cached in
    /// `GitSyncForm::git_available` for the card. The probe used to run
    /// inside `view()`, which froze the UI and flashed a console window
    /// per render on Windows.
    GitAvailabilityChecked(bool),
    ToggleEnabled,
    TogglePasswords,
    ModeChanged(String),
    DeviceNameChanged(String),
    SignalingUrlChanged(String),
    /// Bearer token text-input change. Persisted to the vault settings
    /// table; an empty string leaves the request without an
    /// `Authorization` header (fine for unauthenticated signaling).
    SignalingTokenChanged(super::Redacted),
    RelayUrlChanged(String),
    ListenPortChanged(String),
    // "Set up your own relay" wizard (Settings > Sync > P2P).
    WizardToggle,
    WizardDomainChanged(String),
    WizardPortChanged(String),
    WizardFormatChanged(crate::state::RelayWizardFormat),
    WizardRegenToken,
    WizardTest,
    WizardTestResult(Result<(), String>),
    // "Install on one of your hosts", the wizard's second level (E3):
    // the app SSHes into a vault host, installs the verified relay as a
    // systemd service and adopts the endpoint, after showing every
    // command. Handled in `dispatch_relay_deploy`.
    DeployToggle,
    DeployHostPickerOpen,
    DeployHostPickerClose,
    DeployHostPickerSearch(String),
    DeployHostChanged(uuid::Uuid),
    DeployPortChanged(String),
    DeployCaddyToggled,
    /// Connect, probe the host, fetch + verify the relay binary for its
    /// arch, and render the plan the consent modal shows.
    DeployProbe,
    /// The probe's outcome. Carries the probe's sequence number so an
    /// answer for a flow the user has since abandoned (host changed,
    /// section closed) is dropped instead of reviving it.
    DeployProbed(u64, Result<Box<crate::dispatch_relay_deploy::ProbeOutcome>, crate::dispatch_relay_deploy::ProbeFailure>),
    /// Open the consent modal over the rendered plan.
    DeployReview,
    /// Close the consent modal without running anything.
    DeployConfirmCancel,
    /// The consent was given: upload and run the plan on the host.
    DeployRun,
    /// One log line from the run, token already masked at the source.
    /// `Err` is a step that failed (the run stops after it).
    DeployProgress(u64, crate::relay_deploy::RelayDeployStep, Result<String, String>),
    /// The run ended: `Ok(adopted URL)` or the failure line.
    DeployFinished(u64, Result<String, String>),
    StartPairing,
    UnpairDevice(uuid::Uuid),
    Now,
    /// Top-level result of a manual `SyncNow`. Per-peer outcomes arrive
    /// separately as `SyncEngineEvent`s; this only carries a vault-level
    /// failure (e.g. the lock could not be taken).
    NowFinished(Result<(), String>),
    /// An event emitted by the running `SyncEngine` (peer discovered,
    /// sync completed, pairing progress, ...), pumped in from the
    /// engine's event channel via `Task::stream`.
    EngineEvent(oryxis_sync::SyncEvent),
    /// The off-thread engine spawn (`start_sync_engine`) finished; its
    /// outcome is waiting in `SyncState::pending_engine`. The runtime
    /// and its event receiver aren't Clone, so they cross the task
    /// boundary through a oneshot instead of a message payload.
    EngineSpawned,
    /// Stop hosting the pairing code and return to the idle pairing view.
    CancelHostingPairing,
    /// Switch the pairing panel into "join with a code" mode.
    JoinPairingRequested,
    /// Text-input change for the joiner's 6-digit code field.
    JoinCodeChanged(String),
    /// Text-input change for the joiner's `ip:port` host-address field.
    JoinTargetChanged(String),
    /// Joiner pressed Connect: dial the entered address with the code.
    JoinPairingConnect,
    /// Joiner backed out of the join form, return to the idle view.
    JoinPairingCancel,
    /// Text-input change for the joiner's `oryxis://pair/...` link
    /// field (the cross-network alternative to code + address).
    JoinLinkChanged(String),
    /// Joiner pressed Connect with link: parse the link, look the
    /// device id up on the signaling server, run the handshake.
    JoinPairingByLink,
    /// User clicked Pair on a row in the live discovered-devices
    /// list. Switches to the Joining sub-view and pre-fills the
    /// host-address field with the discovered peer's `ip:port`.
    PairWithDiscovered(uuid::Uuid),
    /// Abort the in-flight `Sync Now` Task. Fires the oneshot the
    /// task is racing against; the task lands back as
    /// `SyncNowFinished(Err("Cancelled"))` and clears the flags.
    CancelInProgress,
    /// Switch the sync transport ("p2p", "sftp", "folder", "git",
    /// "webdav"). Persists the setting, stops the P2P engine when
    /// leaving `p2p`, and starts it when returning. Entering `git` also
    /// re-probes `git` availability off the UI thread (see
    /// `dispatch_git_sync::git_availability_task`).
    TransportChanged(String),
    /// Pick the host the SFTP-sync snapshot file lives on (also closes
    /// the picker modal).
    SftpHostChanged(uuid::Uuid),
    /// Open the "Select a host" modal for the SFTP-sync backup host.
    SftpHostPickerOpen,
    /// Close that modal without changing the selection.
    SftpHostPickerClose,
    /// Search-filter text change inside the host picker modal.
    SftpHostPickerSearch(String),
    /// Text-input change for the SFTP-sync remote path.
    SftpPathChanged(String),
    /// Text-input change for the shared snapshot-transport group
    /// passphrase (SFTP / folder / Git / WebDAV all derive one key from
    /// the same stored row). `Redacted` like every other secret-bearing
    /// variant: a plain `String` would print the group passphrase into
    /// any log line or panic that formats a message (enforced by
    /// `secret_bearing_variants_carry_redacted`). The typed value is the
    /// round's key candidate only; it is NOT persisted until a round
    /// succeeds with it (`commit_sync_passphrase`), so an accidental
    /// keystroke can't swap the group key under the existing snapshot.
    PassphraseChanged(super::Redacted),
    /// Enter the "change passphrase" edit mode for the shared group
    /// secret. With a stored passphrase the field is READ-ONLY (a masked
    /// box): free typing is what let a mistyped re-entry swap the group
    /// key under the existing snapshot. Carries the input's id so the
    /// handler can focus it the moment the editable field appears.
    PassphraseChangeRequested(&'static str),
    /// A mouse press landed while a passphrase edit was open. The handler
    /// first probes the click position against the field's last drawn
    /// bounds (iced does not blur a text input on a button/blank click,
    /// so geometry is the primary signal); when that is inconclusive it
    /// falls back to asking where focus actually went. Only an EMPTY edit
    /// is abandoned, so the subscription can stay mounted for the whole
    /// edit without a condition race.
    PassphraseBlurCheck,
    /// The focus-probe fallback answered: `true` when the passphrase
    /// field still holds iced focus. An empty edit whose field lost
    /// focus is abandoned and the read-only mask returns.
    PassphraseBlurChecked(bool),
    /// Auto-cadence timer fired (any snapshot transport, mode `auto`):
    /// run a round on whichever one is selected, if none is already in
    /// flight. One variant for all of them, because exactly one
    /// transport is active at a time and a per-transport tick would be
    /// four subscriptions racing to say the same thing.
    SnapshotTick,
    /// An SFTP-sync round finished. `Ok` carries a short status summary;
    /// `Err` a human-readable failure. A failed round never overwrites
    /// the remote snapshot (see `dispatch_sftp_sync`).
    SftpDone(Result<String, String>),
}
