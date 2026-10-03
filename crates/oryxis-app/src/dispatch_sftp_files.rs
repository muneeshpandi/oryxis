//! `Oryxis::handle_sftp_files`, match arms for per-file SFTP
//! operations: chmod-style Properties dialog and edit-in-place
//! (download to temp, open in OS editor, mtime-watch + auto-upload).
//! Pulled out of `dispatch_sftp.rs` to keep that file focused on
//! navigation/listing.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, SidebarFilesMessage, Message, Oryxis};

impl Oryxis {
    pub(crate) fn handle_sftp_files(
        &mut self,
        message: SftpMessage,
    ) -> Result<Task<Message>, SftpMessage> {
        use crate::state::SftpPaneSide;
        // Edit-in-place arms carry their originating pane explicitly (a
        // recomputed "remote side" picks the wrong host when both panes
        // are remote); only local-pane conveniences resolve a default.
        let local_side = self.cur_sftp().local_side().unwrap_or(SftpPaneSide::Left);
        match message {
            SftpMessage::SftpShowProperties(side, path, is_dir) => {
                self.sftp.row_menu = None;
                if !self.cur_sftp().pane(side).is_remote {
                        // Local stat is sync, populate the modal in
                        // place. Permissions on Windows are coarser so
                        // Apply will refuse to chmod there (the dialog
                        // still shows file info).
                        let p = std::path::Path::new(&path);
                        let meta = match std::fs::metadata(p) {
                            Ok(m) => m,
                            Err(e) => {
                                self.sftp.pane_mut(side).error = Some(e.to_string());
                                return Ok(Task::none());
                            }
                        };
                        #[cfg(unix)]
                        let mode = {
                            use std::os::unix::fs::MetadataExt as _;
                            meta.mode()
                        };
                        #[cfg(not(unix))]
                        let mode = if meta.permissions().readonly() {
                            0o444
                        } else {
                            0o644
                        };
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as u32);
                        #[cfg(unix)]
                        let (uid, gid) = {
                            use std::os::unix::fs::MetadataExt as _;
                            (Some(meta.uid()), Some(meta.gid()))
                        };
                        #[cfg(not(unix))]
                        let (uid, gid) = (None, None);
                        let view = crate::state::PropertiesView {
                            side,
                            client_override: None,
                            from_sidebar: false,
                            path,
                            is_dir,
                            size: meta.len(),
                            mtime,
                            owner_uid: uid,
                            owner_gid: gid,
                            original_mode: mode,
                            bits: crate::state::PermBits::from_mode(mode),
                            mode_input: format!("{:03o}", mode & 0o777),
                            applying: false,
                            error: None,
                        };
                        self.sftp.properties = Some(view);
                } else {
                        let Some(client) = self.cur_sftp().pane(side).client.clone() else {
                            self.sftp.pane_mut(side).error = Some("Not connected".into());
                            return Ok(Task::none());
                        };
                        let target = path.clone();
                        return Ok(Task::perform(
                            async move {
                                client.stat(&target).await.map_err(|e| e.to_string())
                            },
                            move |result| match result {
                                Ok(stat) => {
                                    let mode = stat.permissions.unwrap_or(0o644);
                                    Message::Sftp(SftpMessage::SftpPropertiesLoaded(crate::state::PropertiesView {
                                        side,
                                        client_override: None,
                                        from_sidebar: false,
                                        path: path.clone(),
                                        is_dir,
                                        size: stat.size,
                                        mtime: stat.mtime,
                                        owner_uid: stat.uid,
                                        owner_gid: stat.gid,
                                        original_mode: mode,
                                        bits: crate::state::PermBits::from_mode(mode),
                                        mode_input: format!("{:03o}", mode & 0o777),
                                        applying: false,
                                        error: None,
                                    }))
                                }
                                Err(e) => Message::Sftp(SftpMessage::SftpOpResult(side, e, true)),
                            },
                        ));
                }
            }
            SftpMessage::SftpPropertiesLoaded(view) => {
                self.sftp.properties = Some(view);
            }
            SftpMessage::SftpPropertiesToggleBit(bit) => {
                if let Some(p) = self.sftp.properties.as_mut() {
                    let b = &mut p.bits;
                    let f = match bit {
                        crate::state::PermBit::UserR => &mut b.user_r,
                        crate::state::PermBit::UserW => &mut b.user_w,
                        crate::state::PermBit::UserX => &mut b.user_x,
                        crate::state::PermBit::GroupR => &mut b.group_r,
                        crate::state::PermBit::GroupW => &mut b.group_w,
                        crate::state::PermBit::GroupX => &mut b.group_x,
                        crate::state::PermBit::OtherR => &mut b.other_r,
                        crate::state::PermBit::OtherW => &mut b.other_w,
                        crate::state::PermBit::OtherX => &mut b.other_x,
                    };
                    *f = !*f;
                    // Keep the numeric field in lockstep with the checkboxes.
                    p.mode_input = format!("{:03o}", p.bits.to_mode());
                }
            }
            SftpMessage::SftpPropertiesModeInput(s) => {
                if let Some(p) = self.sftp.properties.as_mut() {
                    // Accept only octal digits, at most 4 (a leading special-bit
                    // digit is tolerated but ignored: the dialog edits rwx only
                    // and Apply preserves setuid/setgid/sticky from the original).
                    let cleaned: String =
                        s.chars().filter(|c| ('0'..='7').contains(c)).take(4).collect();
                    p.mode_input = cleaned.clone();
                    // Rewrite the checkboxes from a parseable value; leave them
                    // untouched while the field is empty or mid-edit.
                    if let Ok(v) = u32::from_str_radix(&cleaned, 8) {
                        p.bits = crate::state::PermBits::from_mode(v & 0o777);
                    }
                }
            }
            SftpMessage::SftpPropertiesApply => {
                let Some(p) = self.sftp.properties.as_mut() else {
                    return Ok(Task::none());
                };
                if p.applying {
                    return Ok(Task::none());
                }
                // Preserve the high bits (setuid / setgid / sticky)
                // we don't expose for editing, strip rwxrwxrwx out of
                // the original and overlay our edited 9 bits.
                let new_mode = (p.original_mode & !0o777) | p.bits.to_mode();
                if new_mode == p.original_mode {
                    self.sftp.properties = None;
                    return Ok(Task::none());
                }
                p.applying = true;
                p.error = None;
                let path = p.path.clone();
                let side = p.side;
                // Sidebar Files browser target: chmod through its own
                // channel, never an SFTP pane's.
                if let Some(client) = p.client_override.clone() {
                    return Ok(Task::perform(
                        async move {
                            client.chmod(&path, new_mode).await.map_err(|e| e.to_string())
                        },
                        |v| Message::Sftp(SftpMessage::SftpPropertiesDone(v)),
                    ));
                }
                if !self.cur_sftp().pane(side).is_remote {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt as _;
                            let result = std::fs::set_permissions(
                                &path,
                                std::fs::Permissions::from_mode(new_mode),
                            )
                            .map_err(|e| e.to_string());
                            return Ok(Task::done(Message::Sftp(SftpMessage::SftpPropertiesDone(result))));
                        }
                        #[cfg(not(unix))]
                        {
                            return Ok(Task::done(Message::Sftp(SftpMessage::SftpPropertiesDone(Err(
                                "chmod not supported on this platform".into(),
                            )))));
                        }
                } else {
                        let Some(client) = self.cur_sftp().pane(side).client.clone() else {
                            return Ok(Task::done(Message::Sftp(SftpMessage::SftpPropertiesDone(Err(
                                "Not connected".into(),
                            )))));
                        };
                        return Ok(Task::perform(
                            async move {
                                client.chmod(&path, new_mode).await.map_err(|e| e.to_string())
                            },
                            |v| Message::Sftp(SftpMessage::SftpPropertiesDone(v)),
                        ));
                }
            }
            SftpMessage::SftpPropertiesDone(result) => {
                match result {
                    Ok(()) => {
                        let side = self.cur_sftp().properties.as_ref().map(|p| p.side);
                        let from_sidebar = self
                            .sftp
                            .properties
                            .as_ref()
                            .map(|p| p.from_sidebar)
                            .unwrap_or(false);
                        self.sftp.properties = None;
                        // A sidebar-owned chmod refreshes the sidebar
                        // browser (the modal blocked interaction, so the
                        // active pane is still the one that opened it).
                        if from_sidebar {
                            return Ok(self.update(Message::SidebarFiles(SidebarFilesMessage::SidebarFilesRefresh)));
                        }
                        // Refresh whichever pane we just touched so
                        // the new permissions show up immediately.
                        return Ok(match side {
                            Some(side) if !self.cur_sftp().pane(side).is_remote => {
                                self.refresh_sftp_local(side);
                                Task::none()
                            }
                            Some(side) => Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(
                                side,
                                self.cur_sftp().pane(side).remote_path.clone(),
                            ))),
                            None => Task::none(),
                        });
                    }
                    Err(e) => {
                        if let Some(p) = self.sftp.properties.as_mut() {
                            p.applying = false;
                            p.error = Some(e);
                        }
                    }
                }
            }
            SftpMessage::SftpPropertiesClose => {
                self.sftp.properties = None;
            }
            SftpMessage::SftpOpenLocal(path) => {
                self.sftp.row_menu = None;
                if let Err(e) = open::that(&path) {
                    self.sftp.pane_mut(local_side).error = Some(format!(
                        "Failed to open {}: {e}",
                        path.display()
                    ));
                }
            }
            SftpMessage::SftpRevealInExplorer(path, is_dir) => {
                // Reachable from both the row menu and the `⋮` menu.
                self.sftp.close_menus();
                if let Err(e) = crate::util::reveal_in_file_manager(&path, is_dir) {
                    self.sftp.pane_mut(local_side).error = Some(format!(
                        "Failed to open {}: {e}",
                        path.display()
                    ));
                }
            }
            SftpMessage::SftpStartEdit(side, remote_path) => {
                // "Open / Edit": the OS file association, watched in the
                // background like every other opener. The surface stays
                // usable while the local application runs and each save
                // asks whether to send the file back (FileZilla model);
                // nothing about this flow blocks navigation.
                self.sftp.row_menu = None;
                return Ok(self.start_edit_watch(
                    side,
                    remote_path,
                    crate::state::SftpEditOpener::OsDefault,
                    None,
                    false,
                ));
            }
            SftpMessage::SftpEditWatchTick => {
                // Cheap mtime poll on each watched temp file, across EVERY
                // surface so a parked tab's file keeps uploading; a save on a
                // surface that isn't visible still raises the dialog, which
                // layers globally. With an autosave grant the upload starts
                // right here instead of asking.
                let auto = self.prefs.sftp_edit_autosave || self.sftp_edit_upload_all;
                let mut tasks: Vec<Task<Message>> = Vec::new();
                let mut scan = |st: &mut crate::state::SftpState| {
                    for w in st.edit_watches.iter_mut() {
                        if w.uploading {
                            continue;
                        }
                        let Ok(meta) = std::fs::metadata(&w.temp_path) else { continue };
                        let Ok(mtime) = meta.modified() else { continue };
                        match w.initial_mtime {
                            None => w.initial_mtime = Some(mtime),
                            Some(initial) if mtime > initial => {
                                if auto {
                                    w.uploading = true;
                                    w.dirty = false;
                                    tasks.push(watch_upload_task(w.clone(), self.prefs.sftp_upload_temp_name));
                                } else {
                                    w.dirty = true;
                                }
                            }
                            _ => {}
                        }
                    }
                };
                scan(&mut self.sftp);
                for tab in self.sftp_tabs.iter_mut() {
                    scan(&mut tab.state);
                }
                for tab in self.tabs.iter_mut() {
                    scan(&mut tab.files_state);
                }
                if !tasks.is_empty() {
                    return Ok(Task::batch(tasks));
                }
            }
            SftpMessage::SftpStartEditWith(side, remote_path, opener) => {
                self.sftp.close_menus();
                return Ok(self.start_edit_watch(side, remote_path, opener, None, false));
            }
            SftpMessage::SftpToggleOpenGroup => {
                if let Some(menu) = self.sftp.row_menu.as_mut() {
                    menu.open_group = !menu.open_group;
                }
            }
            SftpMessage::SftpPickEditorFor(side, remote_path) => {
                self.sftp.close_menus();
                // Seeded at the configured editor's folder when there is
                // one, so "the other one next to it" is one click away.
                let start = std::path::PathBuf::from(self.prefs.sftp_default_editor.trim())
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|p| p.to_path_buf());
                return Ok(Task::perform(
                    async move {
                        let mut dialog = rfd::AsyncFileDialog::new()
                            .set_title(crate::i18n::t("sftp_open_with_other"));
                        if let Some(dir) = start {
                            dialog = dialog.set_directory(dir);
                        }
                        dialog
                            .pick_file()
                            .await
                            .map(|f| f.path().to_string_lossy().into_owned())
                    },
                    move |app| match app {
                        Some(app) => Message::Sftp(SftpMessage::SftpStartEditWith(
                            side,
                            remote_path.clone(),
                            crate::state::SftpEditOpener::Editor(app),
                        )),
                        // Dismissed: nothing was downloaded, nothing to undo.
                        None => Message::NoOp,
                    },
                ));
            }
            SftpMessage::SftpEditWatchReady(session) => {
                // A second "Open with" of the same remote file REPLACES the
                // old watch, across every surface: two live watches for one
                // path would race their uploads in undefined order under an
                // autosave grant. The superseded editor stays open, its
                // saves just stop uploading (next save via the new watch).
                let same = |w: &crate::state::EditSession| {
                    w.remote_path == session.remote_path && w.host == session.host
                };
                self.sftp.edit_watches.retain(|w| !same(w));
                for tab in self.sftp_tabs.iter_mut() {
                    tab.state.edit_watches.retain(|w| !same(w));
                }
                for tab in self.tabs.iter_mut() {
                    tab.files_state.edit_watches.retain(|w| !same(w));
                }
                self.sftp.edit_watches.push(session);
            }
            SftpMessage::SftpEditPromptChoice(choice, temp_path) => {
                use crate::state::SftpEditPromptChoice as C;
                match choice {
                    C::Yes => {
                        // Addressed by temp file, never "the first dirty
                        // watch": the dialog can be answering for a watch that
                        // belongs to a parked tab.
                        if let Some(w) = self.edit_watch_mut(&temp_path)
                            && !w.uploading
                        {
                            w.uploading = true;
                            w.dirty = false;
                            return Ok(watch_upload_task(w.clone(), self.prefs.sftp_upload_temp_name));
                        }
                    }
                    C::YesToAll | C::Autosave => {
                        if choice == C::Autosave {
                            // Persisted; Settings > SFTP has the toggle to
                            // turn it back off (never a one-way trap).
                            self.prefs.sftp_edit_autosave = true;
                            self.persist_setting("sftp_edit_autosave", "true");
                        } else {
                            self.sftp_edit_upload_all = true;
                        }
                        // Drain every pending save under the fresh grant,
                        // whichever surface it belongs to.
                        let mut tasks: Vec<Task<Message>> = Vec::new();
                        let mut drain = |st: &mut crate::state::SftpState| {
                            for w in st.edit_watches.iter_mut() {
                                if w.dirty && !w.uploading {
                                    w.uploading = true;
                                    w.dirty = false;
                                    tasks.push(watch_upload_task(w.clone(), self.prefs.sftp_upload_temp_name));
                                }
                            }
                        };
                        drain(&mut self.sftp);
                        for tab in self.sftp_tabs.iter_mut() {
                            drain(&mut tab.state);
                        }
                        for tab in self.tabs.iter_mut() {
                            drain(&mut tab.files_state);
                        }
                        return Ok(Task::batch(tasks));
                    }
                    C::No => {
                        if let Some(w) = self.edit_watch_mut(&temp_path) {
                            // Skip this save; re-arm so the NEXT save
                            // prompts again.
                            w.dirty = false;
                            w.initial_mtime = std::fs::metadata(&w.temp_path)
                                .ok()
                                .and_then(|m| m.modified().ok())
                                .or(w.initial_mtime);
                        }
                    }
                    C::Cancel => {
                        // Stop watching this file. The temp copy stays on
                        // disk, the user's editor may still have it open.
                        self.take_edit_watch(&temp_path);
                    }
                }
            }
            SftpMessage::SftpEditReopenChoice(choice) => {
                use crate::state::SftpEditReopenChoice as C;
                let Some(prompt) = self.sftp_edit_reopen.take() else {
                    return Ok(Task::none());
                };
                match choice {
                    C::Reopen => {
                        // Launch the same application on the copy that is
                        // already on disk. The watch (and any save waiting
                        // for an answer) is left exactly as it was.
                        let editor = match &prompt.watch_opener {
                            crate::state::SftpEditOpener::ConfiguredEditor => {
                                Some(self.prefs.sftp_default_editor.trim().to_string())
                            }
                            // The application the watch was launched with,
                            // replayed verbatim: reopening must land back
                            // in the app that already holds the file, not
                            // raise the picker again.
                            crate::state::SftpEditOpener::Editor(path) => Some(path.clone()),
                            _ => None,
                        };
                        let temp = prompt.temp_path.clone();
                        let opener = prompt.watch_opener;
                        return Ok(Task::perform(
                            async move { spawn_edit_opener(&temp, opener, editor.as_deref()) },
                            |result| match result {
                                Ok(()) => Message::NoOp,
                                Err(e) => Message::Sftp(SftpMessage::SftpEditToast(e)),
                            },
                        ));
                    }
                    C::Fresh => {
                        // Drop the watch and the local copy, then run the
                        // open again from scratch. Any unsaved editor buffer
                        // is the user's own call, they were told.
                        self.take_edit_watch(&prompt.temp_path);
                        let _ = std::fs::remove_file(&prompt.temp_path);
                        return Ok(self.start_edit_watch(
                            prompt.side,
                            prompt.remote_path,
                            prompt.opener,
                            Some((prompt.client, prompt.host)),
                            prompt.to_toast,
                        ));
                    }
                    C::Cancel => {}
                }
            }
            SftpMessage::SftpEditToast(text) => {
                return Ok(self.show_toast_secs(text, 5));
            }
            SftpMessage::SftpEditWatchUploadDone(temp_path, result) => {
                // The watch may live on any surface (a parked tab's own
                // state), so it is looked up by temp file like every other
                // watch-addressed message.
                let label = self
                    .edit_watch_mut(&temp_path)
                    .and_then(|w| rearm_watch(w, &result));
                match result {
                    Ok(_) => {
                        if let Some(label) = label {
                            return Ok(self.show_toast_secs(
                                crate::i18n::t("sftp_edit_uploaded")
                                    .replacen("{file}", &label, 1),
                                3,
                            ));
                        }
                    }
                    Err(e) => {
                        // The baseline was re-armed past the failed content,
                        // so nothing retries on its own: tell the user the
                        // way back is another save.
                        return Ok(self.show_toast_secs(
                            format!(
                                "{}: {e}. {}",
                                crate::i18n::t("sftp_edit_upload_failed"),
                                crate::i18n::t("sftp_edit_save_retry")
                            ),
                            6,
                        ));
                    }
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// Shared entry point for every "open this remote file in a local
    /// application" action: the row menu's Open / Edit and Open with
    /// family, and the sidebar Files browser's Edit. Resolves the opener,
    /// refuses early when the configured editor is missing, raises the
    /// reopen dialog when the file is already being edited, and otherwise
    /// downloads a temp copy, launches the application and registers the
    /// background watch.
    ///
    /// `channel` overrides the SFTP channel + host label instead of
    /// resolving them from the pane: the sidebar Files browser has no pane
    /// at all, and the reopen dialog's "download again" replays the pair it
    /// captured. `to_toast` sends failures to a toast rather than a pane's
    /// error banner, for the same paneless callers.
    pub(crate) fn start_edit_watch(
        &mut self,
        side: crate::state::SftpPaneSide,
        remote_path: String,
        opener: crate::state::SftpEditOpener,
        channel: Option<(oryxis_ssh::SftpClient, String)>,
        to_toast: bool,
    ) -> Task<Message> {
        // The configured editor resolves NOW so an unset setting fails with
        // guidance instead of a broken spawn later.
        let editor = match &opener {
            crate::state::SftpEditOpener::ConfiguredEditor => {
                let e = self.prefs.sftp_default_editor.trim().to_string();
                if e.is_empty() {
                    return self.show_toast_secs(
                        crate::i18n::t("sftp_no_editor_configured").to_string(),
                        5,
                    );
                }
                Some(e)
            }
            // Already chosen by hand for this open; only an empty pick
            // (the picker was dismissed) is worth refusing.
            crate::state::SftpEditOpener::Editor(path) => {
                let e = path.trim().to_string();
                if e.is_empty() {
                    return self.show_toast_secs(
                        crate::i18n::t("sftp_no_editor_configured").to_string(),
                        5,
                    );
                }
                Some(e)
            }
            _ => None,
        };
        let (client, host) = match channel {
            Some((client, host)) => (client, host),
            None => {
                let Some(client) = self.cur_sftp().pane(side).client.clone() else {
                    self.sftp.pane_mut(side).error = Some(crate::i18n::t("sftp_not_connected").to_string());
                    return Task::none();
                };
                (client, self.cur_sftp().pane(side).host_label.clone().unwrap_or_default())
            }
        };
        // Already open in a local application: ask instead of overwriting
        // the local copy behind the user's back. A watch whose temp file
        // has since vanished (a cleaned temp dir) is stale, drop it and
        // download fresh.
        if let Some(w) = self.find_edit_watch(&host, &remote_path) {
            // Whether the local copy holds work the server hasn't seen is read
            // from DISK, not from `dirty`: that flag only turns true once the
            // 2s watcher tick has noticed, and the collision is most likely
            // right after a save (save, then immediately reopen), when it is
            // still false. Trusting it would let the discard branch delete a
            // save without ever warning about it.
            let saved_since_download = std::fs::metadata(&w.temp_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .zip(w.initial_mtime)
                .is_some_and(|(now, base)| now > base);
            let (temp_path, label, watch_opener) =
                (w.temp_path.clone(), w.label.clone(), w.opener.clone());
            let pending_save = w.dirty || saved_since_download;
            if temp_path.exists() {
                self.sftp_edit_reopen = Some(crate::state::EditReopenPrompt {
                    temp_path,
                    label,
                    remote_path,
                    host,
                    pending_save,
                    watch_opener,
                    side,
                    opener,
                    client,
                    to_toast,
                });
                return Task::none();
            }
            self.take_edit_watch(&temp_path);
        }
        let watch_client = client.clone();
        Task::perform(
            async move {
                let (basename, temp_path) = crate::util::edit_temp_file(&remote_path);
                let bytes = client
                    .read_file(&remote_path)
                    .await
                    .map_err(|e| e.to_string())?;
                tokio::fs::write(&temp_path, &bytes)
                    .await
                    .map_err(|e| format!("write temp: {e}"))?;
                // Tighten temp file perms to 0600: the file holds plaintext
                // remote contents and shouldn't be world-readable on a
                // shared system. Default umask often leaves files at 0644.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    let _ = tokio::fs::set_permissions(
                        &temp_path,
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .await;
                }
                spawn_edit_opener(&temp_path, opener.clone(), editor.as_deref())?;
                let initial_mtime = tokio::fs::metadata(&temp_path)
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok());
                Ok::<crate::state::EditSession, String>(crate::state::EditSession {
                    client: Some(watch_client),
                    remote_path,
                    temp_path,
                    label: basename,
                    host,
                    opener,
                    initial_mtime,
                    dirty: false,
                    uploading: false,
                })
            },
            move |result| match result {
                Ok(session) => Message::Sftp(SftpMessage::SftpEditWatchReady(session)),
                // A paneless caller (sidebar) has no error banner to fill.
                Err(e) if to_toast => Message::Sftp(SftpMessage::SftpEditToast(e)),
                Err(e) => Message::Sftp(SftpMessage::SftpOpResult(side, e, true)),
            },
        )
    }

    /// Live watch for `(host, remote_path)` on ANY surface: the hoisted
    /// buffer, a parked SFTP tab, or a terminal tab's Files-mode state.
    /// One remote file is only ever watched once, so the first hit is it.
    fn find_edit_watch(
        &self,
        host: &str,
        remote_path: &str,
    ) -> Option<&crate::state::EditSession> {
        let same = |w: &&crate::state::EditSession| w.host == host && w.remote_path == remote_path;
        self.sftp
            .edit_watches
            .iter()
            .find(same)
            .or_else(|| self.sftp_tabs.iter().find_map(|t| t.state.edit_watches.iter().find(same)))
            .or_else(|| {
                self.tabs
                    .iter()
                    .find_map(|t| t.files_state.edit_watches.iter().find(same))
            })
    }

    /// Mutable watch owning `temp_path`, wherever it lives. Temp paths are
    /// unique per open, so this is the stable identity for the dialogs and
    /// the upload completions.
    fn edit_watch_mut(
        &mut self,
        temp_path: &std::path::Path,
    ) -> Option<&mut crate::state::EditSession> {
        if let Some(pos) = self
            .sftp
            .edit_watches
            .iter()
            .position(|w| w.temp_path == temp_path)
        {
            return self.sftp.edit_watches.get_mut(pos);
        }
        if let Some(w) = self.sftp_tabs.iter_mut().find_map(|t| {
            t.state
                .edit_watches
                .iter_mut()
                .find(|w| w.temp_path == temp_path)
        }) {
            return Some(w);
        }
        self.tabs.iter_mut().find_map(|t| {
            t.files_state
                .edit_watches
                .iter_mut()
                .find(|w| w.temp_path == temp_path)
        })
    }

    /// Stop watching the file at `temp_path` and hand the entry back. The
    /// temp copy is left on disk: the user's editor may still hold it.
    fn take_edit_watch(
        &mut self,
        temp_path: &std::path::Path,
    ) -> Option<crate::state::EditSession> {
        let pick = |list: &mut Vec<crate::state::EditSession>| {
            list.iter()
                .position(|w| w.temp_path == temp_path)
                .map(|pos| list.remove(pos))
        };
        if let Some(w) = pick(&mut self.sftp.edit_watches) {
            return Some(w);
        }
        for tab in self.sftp_tabs.iter_mut() {
            if let Some(w) = pick(&mut tab.state.edit_watches) {
                return Some(w);
            }
        }
        for tab in self.tabs.iter_mut() {
            if let Some(w) = pick(&mut tab.files_state.edit_watches) {
                return Some(w);
            }
        }
        None
    }

    /// Dismiss the save dialog without a button press (Esc, or the modal
    /// being closed out from under it): skip THIS save and re-arm the
    /// baseline so the next one prompts again. Never uploads.
    pub(crate) fn skip_pending_edit_save(&mut self) {
        let Some(temp) = self.pending_edit_save().map(|w| w.temp_path.clone()) else {
            return;
        };
        if let Some(w) = self.edit_watch_mut(&temp) {
            w.dirty = false;
            w.initial_mtime = std::fs::metadata(&w.temp_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .or(w.initial_mtime);
        }
    }

    /// The save waiting for an answer, on ANY surface. Drives the save
    /// dialog, which layers globally so a save on a parked tab is still
    /// resolvable (before this it could sit dirty forever, invisible).
    pub(crate) fn pending_edit_save(&self) -> Option<&crate::state::EditSession> {
        let ready = |w: &&crate::state::EditSession| w.dirty && !w.uploading;
        self.sftp
            .edit_watches
            .iter()
            .find(ready)
            .or_else(|| {
                self.sftp_tabs
                    .iter()
                    .find_map(|t| t.state.edit_watches.iter().find(ready))
            })
            .or_else(|| {
                self.tabs
                    .iter()
                    .find_map(|t| t.files_state.edit_watches.iter().find(ready))
            })
    }
}

/// Apply an upload completion to its watch entry: clear the in-flight
/// flag, re-arm the mtime baseline, and hand back the label on success
/// (for the toast). On error the save is NOT remote; re-arming to the
/// current temp mtime avoids a retry loop while the surfaced toast tells
/// the user their next save will prompt again.
fn rearm_watch(
    w: &mut crate::state::EditSession,
    result: &Result<std::time::SystemTime, String>,
) -> Option<String> {
    w.uploading = false;
    w.dirty = false;
    match result {
        Ok(mtime) => {
            w.initial_mtime = Some(*mtime);
            Some(w.label.clone())
        }
        Err(_) => {
            w.initial_mtime = std::fs::metadata(&w.temp_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .or(w.initial_mtime);
            None
        }
    }
}

/// Spawn the local application for an "Open with" edit, per opener kind.
/// Runs inside the download task (blocking spawn is fine there).
fn spawn_edit_opener(
    temp_path: &std::path::Path,
    opener: crate::state::SftpEditOpener,
    editor: Option<&str>,
) -> Result<(), String> {
    match opener {
        crate::state::SftpEditOpener::OsDefault => open::that(temp_path)
            .map_err(|e| format!("open editor: {e} (temp at {})", temp_path.display())),
        // Both spell "run this command line on the temp file"; they only
        // differ in where the caller read the command from.
        crate::state::SftpEditOpener::ConfiguredEditor
        | crate::state::SftpEditOpener::Editor(_) => {
            let editor = editor.ok_or("no editor configured")?;
            let parts = split_command_line(editor);
            let (program, args) = parts.split_first().ok_or("no editor configured")?;
            std::process::Command::new(program)
                .args(args)
                .arg(temp_path)
                .spawn()
                .map(reap_detached)
                .map_err(|e| format!("spawn {program}: {e}"))
        }
        crate::state::SftpEditOpener::AskOs => open_with_os_picker(temp_path),
    }
}

/// Split a configured editor command line into program + arguments,
/// honoring single/double quotes so quoted paths with spaces work
/// (`"C:\Program Files\VS Code\Code.exe" --wait`). Backslashes are kept
/// literal on purpose: they are path separators on Windows, and treating
/// them as escapes would break every configured Windows editor.
fn split_command_line(cmd: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    parts.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            None => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        parts.push(cur);
    }
    parts
}

/// Hand a spawned child to a reaper thread so it never lingers as a
/// zombie on Unix once the user closes their editor. The thread parks
/// in `wait()` and costs nothing while the editor runs.
fn reap_detached(child: std::process::Child) {
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
}

/// OS "open with" application picker. Windows has a shell verb for it;
/// macOS gets an AppleScript chooser; Linux has no stable cross-desktop
/// picker CLI, so the menu entry is hidden there (this arm is a fallback).
fn open_with_os_picker(temp_path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("rundll32.exe")
            .arg("shell32.dll,OpenAs_RunDLL")
            .arg(temp_path)
            .spawn()
            .map(reap_detached)
            .map_err(|e| format!("open-with dialog: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        // `choose application` returns e.g. "application \"TextEdit\"";
        // `open -a` accepts the display name. The temp path reaches the
        // script strictly as `argv` — the file name comes from the remote
        // server, and interpolating it into the script source would let a
        // hostile name break the string literal and run arbitrary
        // AppleScript (`do shell script`) locally.
        let script = "on run argv\n\
             tell application \"System Events\" to activate\n\
             set theApp to name of (choose application)\n\
             do shell script \"open -a \" & quoted form of theApp & \" \" & quoted form of (item 1 of argv)\n\
             end run";
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .arg(temp_path)
            .spawn()
            .map(reap_detached)
            .map_err(|e| format!("open-with dialog: {e}"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = temp_path;
        Err("The OS application picker is not available on this platform".to_string())
    }
}


/// Upload a watch entry's temp file to its remote path, resolving the
/// temp mtime FIRST so the re-arm covers exactly the content uploaded
/// (a save landing mid-upload keeps a newer mtime and prompts again).
///
/// Routes through `upload_one` like every other upload, so the
/// scratch-name setting applies here too: a dropped connection mid-save
/// leaves the `.oryxis-part` scratch rather than a truncated remote
/// file (a bare `write_file` opens WRITE|CREATE|TRUNCATE under the real
/// name, destroying the file it was saving).
fn watch_upload_task(session: crate::state::EditSession, temp_name: bool) -> Task<Message> {
    let temp_key = session.temp_path.clone();
    Task::perform(
        async move {
            let mtime = tokio::fs::metadata(&session.temp_path)
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or_else(std::time::SystemTime::now);
            let client = session
                .client
                .clone()
                .ok_or_else(|| "no client attached to the edit watch".to_string())?;
            crate::sftp_helpers::upload_one(
                &client,
                std::path::Path::new(&session.temp_path),
                &session.remote_path,
                temp_name,
                None,
            )
            .await?;
            Ok::<std::time::SystemTime, String>(mtime)
        },
        move |result| {
            Message::Sftp(SftpMessage::SftpEditWatchUploadDone(temp_key.clone(), result))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::split_command_line;

    #[test]
    fn split_plain_words() {
        assert_eq!(split_command_line("code --wait"), vec!["code", "--wait"]);
    }

    #[test]
    fn split_keeps_quoted_spaces_and_backslashes() {
        assert_eq!(
            split_command_line(r#""C:\Program Files\VS Code\Code.exe" --wait"#),
            vec![r"C:\Program Files\VS Code\Code.exe", "--wait"]
        );
        assert_eq!(
            split_command_line("'my editor' -n"),
            vec!["my editor", "-n"]
        );
    }

    #[test]
    fn split_empty_and_whitespace() {
        assert!(split_command_line("").is_empty());
        assert!(split_command_line("   ").is_empty());
        // An empty quoted pair still yields an (empty) argument.
        assert_eq!(split_command_line("vim \"\""), vec!["vim", ""]);
    }
}
