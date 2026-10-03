//! Starting a transfer for a FOLDER or a multi-row selection.
//!
//! Same shape as `single`, one walk deeper: the tree (or the selection)
//! is walked into a queue first, so the runner sees the same
//! `TransferState` either way and the conflict prompt can say "apply to
//! remaining".

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::sftp_helpers::{
    build_client_pool, ensure_local_space, is_safe_remote_entry_name, parent_path,
    remote_cp, remote_join,
    unique_name_in_local_dir, unique_name_in_remote_dir, walk_local_for_duplicate,
    walk_local_for_upload, walk_remote_for_download,
};
use super::SftpSides;

impl Oryxis {
    pub(super) fn handle_sftp_batch(
        &mut self,
        message: SftpMessage,
        sides: SftpSides,
    ) -> Result<Task<Message>, SftpMessage> {
        let SftpSides { remote: remote_side, local: local_side, owner } = sides;
        match message {
            SftpMessage::SftpUploadFolder(local_root) => {
                self.sftp.row_menu = None;
                if self.sftp_upload_blocked_by_zip(remote_side) {
                    return Ok(Task::none());
                }
                let Some(client) = self.cur_sftp().pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some(crate::i18n::t("sftp_not_connected").to_string());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.cur_sftp().pane(remote_side).remote_path.clone());
                let concurrency = self.sftp_concurrency();
                Ok(Task::perform(
                    async move {
                        let basename = local_root
                            .file_name()
                            .and_then(|s| s.to_str())
                            .ok_or_else(|| "invalid folder name".to_string())?
                            .to_string();
                        let unique =
                            unique_name_in_remote_dir(&client, &remote_dir, &basename).await?;
                        let target_root = remote_join(&remote_dir, &unique);
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: local_root.to_string_lossy().into_owned(),
                            dst: target_root.clone(),
                            is_dir: true,
                            size: None,
                        });
                        walk_local_for_upload(&local_root, &target_root, &mut queue)
                            .map_err(|e| e.to_string())?;
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Upload,
                            unique,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::Sftp(SftpMessage::SftpTransferQueueReady(owner, state)),
                        Err(e) => Message::Sftp(SftpMessage::SftpOpResult(remote_side, e, true)),
                    },
                ))
            }
            SftpMessage::SftpDownloadFolder(remote_root) => {
                self.sftp.row_menu = None;
                if let Some(ask) = self.sftp_ask_download_dir(SftpMessage::SftpDownloadFolder(
                    remote_root.clone(),
                )) {
                    return Ok(ask);
                }
                let Some(client) = self.cur_sftp().pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some(crate::i18n::t("sftp_not_connected").to_string());
                    return Ok(Task::none());
                };
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.cur_sftp().pane(local_side).local_path.clone());
                let concurrency = self.sftp_concurrency();
                Ok(Task::perform(
                    async move {
                        let basename = remote_root
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&remote_root)
                            .to_string();
                        // The folder's own listing name becomes the local
                        // tree root, so it owes the same one-component rule
                        // the walk enforces on every child below it. Without
                        // this the child guard is moot: the children are
                        // joined onto a root that already escaped.
                        // `unique_name_in_local_dir` does NOT cover it, it is
                        // a collision check against `read_dir` output, and a
                        // separator-bearing name can never appear there, so
                        // it always reads as "free" and passes through.
                        if !is_safe_remote_entry_name(&basename) {
                            return Err(format!(
                                "{} ({basename})",
                                crate::i18n::t("sftp_unsafe_entry_name")
                            ));
                        }
                        // Pick a non-colliding local name.
                        let unique = unique_name_in_local_dir(&local_dir, &basename);
                        let target_root = local_dir.join(&unique);
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: remote_root.clone(),
                            dst: target_root.to_string_lossy().into_owned(),
                            is_dir: true,
                            size: None,
                        });
                        walk_remote_for_download(&client, &remote_root, &target_root, &mut queue)
                            .await?;
                        // The walk collected every file's size, so the
                        // whole tree is measurable before the first byte
                        // lands. A folder is where this matters most: the
                        // remote peer picks both the sizes and the count.
                        ensure_local_space(
                            &local_dir,
                            queue.iter().filter_map(|i| i.size).sum::<u64>(),
                        )?;
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Download,
                            unique,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::Sftp(SftpMessage::SftpTransferQueueReady(owner, state)),
                        Err(e) => Message::Sftp(SftpMessage::SftpOpResult(remote_side, e, true)),
                    },
                ))
            }
            SftpMessage::SftpDuplicateFolder(side, path) => {
                self.sftp.row_menu = None;
                if !self.cur_sftp().pane(side).is_remote {
                        let src = std::path::PathBuf::from(&path);
                        let parent = match src.parent() {
                            Some(p) => p.to_path_buf(),
                            None => {
                                self.sftp.pane_mut(side).error = Some("Cannot duplicate root".into());
                                return Ok(Task::none());
                            }
                        };
                        let basename = src
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("untitled")
                            .to_string();
                        let unique = unique_name_in_local_dir(&parent, &basename);
                        let target_root = parent.join(&unique);
                        // Build the queue synchronously, no client needed
                        // for a local-only walk + copy.
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: src.to_string_lossy().into_owned(),
                            dst: target_root.to_string_lossy().into_owned(),
                            is_dir: true,
                            size: None,
                        });
                        if let Err(e) = walk_local_for_duplicate(&src, &target_root, &mut queue) {
                            self.sftp.pane_mut(side).error = Some(e);
                            return Ok(Task::none());
                        }
                        // Local duplicate uses sync std::fs::copy in
                        // the queue runner, no SFTP channels needed,
                        // so the client pool stays empty. Concurrency
                        // is fixed at 1 for the same reason: spawning
                        // multiple sync workers wouldn't help (they'd
                        // hammer the OS file cache from the same
                        // thread).
                        let state = crate::state::TransferState::new(
                            crate::state::TransferKind::DuplicateLocal,
                            unique,
                            queue,
                            Vec::new(),
                            None,
                            None,
                            1,
                        );
                        Ok(Task::done(Message::Sftp(SftpMessage::SftpTransferQueueReady(owner, state))))
                } else {
                        let Some(client) = self.cur_sftp().pane(side).client.clone() else {
                            return Ok(Task::none());
                        };
                        let parent = parent_path(&path);
                        let basename = path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&path)
                            .to_string();
                        let reload = self.cur_sftp().pane(side).remote_path.clone();
                        let src = path.clone();
                        // `cp -r --`, single fast call, no progress bar
                        // needed since the user can't usefully observe
                        // partial recursive copy progress over SSH anyway.
                        Ok(Task::perform(
                            async move {
                                let unique =
                                    unique_name_in_remote_dir(&client, &parent, &basename)
                                        .await?;
                                let dest = remote_join(&parent, &unique);
                                remote_cp(&client, &src, &dest, true).await?;
                                Ok::<String, String>(reload)
                            },
                            move |result| match result {
                                Ok(reload) => Message::Sftp(SftpMessage::SftpNavigateRemote(side, reload)),
                                Err(e) => Message::Sftp(SftpMessage::SftpOpResult(side, e, true)),
                            },
                        ))
                }
            }
            SftpMessage::SftpUploadBatch(paths) => {
                self.sftp.row_menu = None;
                if self.sftp_upload_blocked_by_zip(remote_side) {
                    return Ok(Task::none());
                }
                if paths.is_empty() {
                    return Ok(Task::none());
                }
                let Some(client) = self.cur_sftp().pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some(crate::i18n::t("sftp_not_connected").to_string());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.cur_sftp().pane(remote_side).remote_path.clone());
                let concurrency = self.sftp_concurrency();
                Ok(Task::perform(
                    async move {
                        let mut queue = std::collections::VecDeque::new();
                        // Each top-level path goes in as-is; folders
                        // expand recursively. Names aren't pre-uniqued
                        //, the per-item conflict check at the queue
                        // runner handles that with user input.
                        for path in &paths {
                            let basename = path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("file")
                                .to_string();
                            let target = if remote_dir == "/" {
                                format!("/{}", basename)
                            } else {
                                format!(
                                    "{}/{}",
                                    remote_dir.trim_end_matches('/'),
                                    basename
                                )
                            };
                            if path.is_dir() {
                                queue.push_back(crate::state::TransferItem {
                                    src: path.to_string_lossy().into_owned(),
                                    dst: target.clone(),
                                    is_dir: true,
                                    size: None,
                                });
                                walk_local_for_upload(path, &target, &mut queue)
                                    .map_err(|e| e.to_string())?;
                            } else {
                                queue.push_back(crate::state::TransferItem {
                                    src: path.to_string_lossy().into_owned(),
                                    dst: target,
                                    is_dir: false,
                                    // Byte size up front so the total is known
                                    // and the bar advances by bytes.
                                    size: path.metadata().map(|m| m.len()).ok(),
                                });
                            }
                        }
                        let label = if paths.len() == 1 {
                            paths[0]
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("upload")
                                .to_string()
                        } else {
                            format!("{} items", paths.len())
                        };
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Upload,
                            label,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::Sftp(SftpMessage::SftpTransferQueueReady(owner, state)),
                        Err(e) => Message::Sftp(SftpMessage::SftpOpResult(remote_side, e, true)),
                    },
                ))
            }
            SftpMessage::SftpUploadSelection => {
                self.sftp.row_menu = None;
                let paths: Vec<std::path::PathBuf> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| !self.cur_sftp().pane(*s).is_remote)
                    .map(|(_, p)| std::path::PathBuf::from(p))
                    .collect();
                if paths.is_empty() {
                    return Ok(Task::none());
                }
                Ok(Task::done(Message::Sftp(SftpMessage::SftpUploadBatch(paths))))
            }
            SftpMessage::SftpDownloadSelection => {
                self.sftp.row_menu = None;
                let remote_items: Vec<(String, bool)> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| self.cur_sftp().pane(*s).is_remote)
                    .map(|(s, p)| (p.clone(), self.row_is_dir_in_pane(*s, p)))
                    .collect();
                if remote_items.is_empty() {
                    return Ok(Task::none());
                }
                Ok(Task::done(Message::Sftp(SftpMessage::SftpDownloadBatch(remote_items))))
            }
            SftpMessage::SftpDownloadBatch(remote_items) => {
                self.sftp.row_menu = None;
                if remote_items.is_empty() {
                    return Ok(Task::none());
                }
                if let Some(ask) =
                    self.sftp_ask_download_dir(SftpMessage::SftpDownloadBatch(remote_items.clone()))
                {
                    return Ok(ask);
                }
                let Some(client) = self.cur_sftp().pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some(crate::i18n::t("sftp_not_connected").to_string());
                    return Ok(Task::none());
                };
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.cur_sftp().pane(local_side).local_path.clone());
                let concurrency = self.sftp_concurrency();
                Ok(Task::perform(
                    async move {
                        let mut queue = std::collections::VecDeque::new();
                        let mut skipped: Vec<String> = Vec::new();
                        for (remote_path, is_dir) in &remote_items {
                            let basename = remote_path
                                .rsplit('/')
                                .find(|s| !s.is_empty())
                                .unwrap_or(remote_path)
                                .to_string();
                            // Skip rather than fail: the list can hold rows
                            // the user picked by hand, so one hostile name
                            // must not sink the others. Same answer the walk
                            // gives per entry.
                            if !is_safe_remote_entry_name(&basename) {
                                tracing::warn!(
                                    "sftp download: skipping unsafe entry name {basename:?} in {remote_path}"
                                );
                                skipped.push(basename);
                                continue;
                            }
                            let target = local_dir.join(&basename);
                            if *is_dir {
                                queue.push_back(crate::state::TransferItem {
                                    src: remote_path.clone(),
                                    dst: target.to_string_lossy().into_owned(),
                                    is_dir: true,
                                    size: None,
                                });
                                walk_remote_for_download(
                                    &client,
                                    remote_path,
                                    &target,
                                    &mut queue,
                                )
                                .await?;
                            } else {
                                queue.push_back(crate::state::TransferItem {
                                    src: remote_path.clone(),
                                    dst: target.to_string_lossy().into_owned(),
                                    is_dir: false,
                                    // The walk carries a size for everything
                                    // under a selected DIRECTORY; a file
                                    // picked at top level owes its own, and
                                    // it is what the space check sums and
                                    // what makes the progress bar advance by
                                    // BYTES instead of by item count (a
                                    // single-file transfer would otherwise
                                    // sit at 0 / 1 for its whole run).
                                    // `ok()` and not `unwrap_or(0)`: a failed
                                    // stat means UNKNOWN, and `Some(0)` would
                                    // read as an empty file, sending a 49 MB
                                    // download down the small-file path with
                                    // no resume and a bar already at 100%.
                                    size: client.stat(remote_path).await.ok().map(|s| s.size),
                                });
                            }
                        }
                        if queue.is_empty() {
                            // Every entry was skipped above. A skip is quiet
                            // while other rows still transfer, but a download
                            // where NOTHING happens owes the reason: this is
                            // the whole answer a single-file download gets.
                            return Err(format!(
                                "{} ({})",
                                crate::i18n::t("sftp_unsafe_entry_name"),
                                skipped.join(", ")
                            ));
                        }
                        ensure_local_space(
                            &local_dir,
                            queue.iter().filter_map(|i| i.size).sum::<u64>(),
                        )?;
                        let label = if remote_items.len() == 1 {
                            remote_items[0]
                                .0
                                .rsplit('/')
                                .find(|s| !s.is_empty())
                                .unwrap_or(&remote_items[0].0)
                                .to_string()
                        } else {
                            format!("{} items", remote_items.len())
                        };
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Download,
                            label,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::Sftp(SftpMessage::SftpTransferQueueReady(owner, state)),
                        Err(e) => Message::Sftp(SftpMessage::SftpOpResult(remote_side, e, true)),
                    },
                ))
            }
            SftpMessage::SftpDuplicateSelection => {
                self.sftp.row_menu = None;
                // Fan out per-item duplicate. They run sequentially
                // anyway because the SFTP connection serializes; for
                // local-side they're independent fs::copy calls.
                let items: Vec<(crate::state::SftpPaneSide, String, bool)> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .map(|(side, path)| (*side, path.clone(), self.row_is_dir_in_pane(*side, path)))
                    .collect();
                if items.is_empty() {
                    return Ok(Task::none());
                }
                let mut tasks = Vec::with_capacity(items.len());
                for (side, path, is_dir) in items {
                    tasks.push(Task::done(if is_dir {
                        Message::Sftp(SftpMessage::SftpDuplicateFolder(side, path))
                    } else {
                        Message::Sftp(SftpMessage::SftpDuplicate(side, path))
                    }));
                }
                self.sftp.selected_rows.clear();
                Ok(Task::batch(tasks))
            }
            // Every arm above returns, so the match IS the return
            // value here rather than a statement before one.
            m => Err(m),
        }
    }
}
