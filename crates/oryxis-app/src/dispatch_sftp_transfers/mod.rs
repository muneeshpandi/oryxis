//! `Oryxis::handle_sftp_transfers`, match arms for the SFTP transfer
//! pipeline: single + batch + folder uploads/downloads/duplicates,
//! conflict resolution, OS-level file drop, queue lifecycle (slots,
//! retry, error reporting, cancel). Pulled out of `dispatch_sftp.rs`
//! since the queue runner is genuinely a different subsystem from the
//! navigation/listing arms.
//!
//! The handler used to be one 1224-line `match`. It is now a router over
//! six sub-handlers, one per phase of a transfer's life, following the
//! domain-router convention: every variant is listed here, each group
//! delegates, and a variant filed under the wrong group surfaces through
//! `unrouted` instead of being silently dropped.

#![allow(clippy::result_large_err)]

mod batch;
mod conflict;
mod drops;
mod queue;
mod relay;
mod single;

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::sftp_helpers::{
    destinations_are_one_directory, parent_path,
    relay_target_is_inside_source, remote_join, resolved_path,
    unique_name_in_remote_dir, walk_remote_for_relay,
};
use crate::state::SftpPaneSide;

/// The pane roles and the owning tab, resolved once per message.
///
/// Every sub-handler needs all three, and resolving them in the router
/// keeps the "no SFTP tab means decline" gate in exactly one place: it
/// is the first handler in the dispatch chain, so answering `Ok` with no
/// tab open would swallow every message in the app.
#[derive(Clone, Copy)]
pub(crate) struct SftpSides {
    /// The remote source/destination pane. Both paths only run with
    /// exactly one remote pane, so this resolves unambiguously.
    pub(crate) remote: SftpPaneSide,
    pub(crate) local: SftpPaneSide,
    /// Owning SFTP tab for any continuation this handler emits: the
    /// focused tab for a user action, the originating one for a routed
    /// continuation (`route_sftp_async`), so a chain stays pinned to the
    /// tab that started it.
    pub(crate) owner: uuid::Uuid,
}

impl Oryxis {
    /// Folder picker for a download destination, seeded at the local
    /// pane's current directory. Returns the task that asks and then
    /// replays `then`; `None` means "no need to ask, go ahead".
    ///
    /// Called at the top of every download entry point, gated on the
    /// `sftp_ask_download_dir` setting. The `download_dest_override`
    /// check is what stops the replay from asking a second time: the
    /// picker sets it, the handler consumes it.
    pub(crate) fn sftp_ask_download_dir(
        &self,
        then: SftpMessage,
    ) -> Option<Task<Message>> {
        if !self.prefs.sftp_ask_download_dir || self.cur_sftp().download_dest_override.is_some() {
            return None;
        }
        Some(self.sftp_pick_download_dir(then))
    }

    /// Unconditional version, behind the row menu's "Download to...".
    pub(crate) fn sftp_pick_download_dir(&self, then: SftpMessage) -> Task<Message> {
        let start = self
            .sftp
            .local_side()
            .map(|s| self.cur_sftp().pane(s).local_path.clone());
        Task::perform(
            async move {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_title(crate::i18n::t("sftp_download_to"));
                if let Some(dir) = start {
                    dialog = dialog.set_directory(dir);
                }
                dialog.pick_folder().await.map(|f| f.path().to_path_buf())
            },
            move |dir| {
                Message::Sftp(SftpMessage::SftpDownloadDestPicked(
                    dir,
                    Box::new(then.clone()),
                ))
            },
        )
    }

    pub(crate) fn handle_sftp_transfers(
        &mut self,
        message: SftpMessage,
    ) -> Result<Task<Message>, SftpMessage> {
        // A terminal / sidebar Files drop conflict's modal answers land
        // here even with no SFTP tab open: the resume targets the
        // pane's paused drop upload, not any SFTP surface. The owner
        // gate below would otherwise decline them into `Task::none` and
        // the upload would stay paused forever.
        match &message {
            SftpMessage::SftpToggleApplyToAll
                if self
                    .sftp
                    .overwrite_prompt
                    .as_ref()
                    .is_some_and(|p| p.drop_upload_pane.is_some()) =>
            {
                if let Some(p) = self.sftp.overwrite_prompt.as_mut() {
                    p.apply_to_all = !p.apply_to_all;
                }
                return Ok(Task::none());
            }
            SftpMessage::SftpResolveOverwrite(action)
                if self
                    .sftp
                    .overwrite_prompt
                    .as_ref()
                    .is_some_and(|p| p.drop_upload_pane.is_some()) =>
            {
                let prompt = self.sftp.overwrite_prompt.take();
                let pane_id = prompt.as_ref().and_then(|p| p.drop_upload_pane);
                let apply_to_all = prompt.map(|p| p.apply_to_all).unwrap_or(false);
                if let Some(pane_id) = pane_id {
                    return Ok(self.resolve_terminal_drop_conflict(
                        pane_id,
                        *action,
                        apply_to_all,
                    ));
                }
            }
            _ => {}
        }
        let Some(owner) = self.current_sftp_owner() else {
            return Err(message);
        };
        let sides = SftpSides {
            remote: self.cur_sftp().remote_side().unwrap_or(SftpPaneSide::Right),
            local: self.cur_sftp().local_side().unwrap_or(SftpPaneSide::Left),
            owner,
        };
        Ok(match message {
            m @ (SftpMessage::SftpDownload(..)
            | SftpMessage::SftpDownloadTo(..)
            | SftpMessage::SftpDownloadDestPicked(..)
            | SftpMessage::SftpDuplicate(..)) => self
                .handle_sftp_single(m, sides)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (SftpMessage::SftpUploadFolder(..)
            | SftpMessage::SftpDownloadFolder(..)
            | SftpMessage::SftpDuplicateFolder(..)
            | SftpMessage::SftpUploadBatch(..)
            | SftpMessage::SftpDownloadBatch(..)
            | SftpMessage::SftpUploadSelection
            | SftpMessage::SftpDownloadSelection
            | SftpMessage::SftpDuplicateSelection) => self
                .handle_sftp_batch(m, sides)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (SftpMessage::SftpToggleApplyToAll
            | SftpMessage::SftpResolveOverwrite(..)
            | SftpMessage::SftpTransferConflict(..)) => self
                .handle_sftp_conflict(m, sides)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (SftpMessage::SftpTransferQueueReady(..)
            | SftpMessage::SftpTransferNext(..)
            | SftpMessage::SftpTransferItemDone(..)
            | SftpMessage::SftpTransferError(..)
            | SftpMessage::SftpCancelTransfer
            | SftpMessage::SftpTransferTick
            | SftpMessage::SftpToggleTransferPanel) => self
                .handle_sftp_queue(m, sides)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (SftpMessage::SftpFileHovered
            | SftpMessage::SftpFilesHoveredLeft
            | SftpMessage::SftpFileDropped(..)
            | SftpMessage::SftpDropFlush) => self
                .handle_sftp_drops(m, sides)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (SftpMessage::SftpRelay(..)
            | SftpMessage::SftpRelayFolder(..)
            | SftpMessage::SftpRelayMove(..)
            | SftpMessage::SftpRelayMoveFolder(..)) => self
                .handle_sftp_relay(m, sides)
                .unwrap_or_else(crate::dispatch::unrouted),
            // Not ours. This handler runs FIRST in the SFTP chain, so
            // anything it does not own has to travel on: answering `Ok`
            // here would swallow every other SFTP message in the app.
            m => return Err(m),
        })
    }
    /// Build and start a server-to-server transfer: the file or tree at
    /// `src_path` on the `from` pane's host, onto the other pane's host.
    ///
    /// One builder for all four entry points, because a move IS a relay
    /// plus a removal at the end. Splitting them would give the move its
    /// own copy of the destination-naming and tree-walking logic, which
    /// is exactly the logic that must not drift between the two.
    ///
    /// `move_source` only attaches the removal list; nothing here
    /// deletes anything. The removal runs from the finalize arm, which
    /// is unreachable unless every item copied AND verified.
    fn start_relay(
        &mut self,
        owner: uuid::Uuid,
        from: SftpPaneSide,
        src_path: String,
        is_dir: bool,
        move_source: bool,
    ) -> Task<Message> {
        self.sftp.row_menu = None;
        let dest_side = if from == SftpPaneSide::Left {
            SftpPaneSide::Right
        } else {
            SftpPaneSide::Left
        };
        let (Some(src_client), Some(dst_client)) = (
            self.cur_sftp().pane(from).client.clone(),
            self.cur_sftp().pane(dest_side).client.clone(),
        ) else {
            self.sftp.pane_mut(from).error = Some(crate::i18n::t("sftp_both_panes_connected").to_string());
            return Task::none();
        };
        // Same machine? Two conservative signals: a shared SSH session is
        // exact (every client from one session holds that session's own
        // handle), and equal host labels mean the panes were mounted from
        // the same vault entry. Neither can claim "same host" for two
        // genuinely different machines, so the containment guard below
        // can only ever fire on paths that really do share a filesystem.
        // The converse is allowed to be wrong: missing a case leaves
        // today's behaviour, while a false positive would refuse a
        // legitimate transfer.
        let same_host = src_client.shares_session_with(&dst_client) || {
            let a = self.cur_sftp().pane(from).host_label.as_ref();
            let b = self.cur_sftp().pane(dest_side).host_label.as_ref();
            a.is_some() && a == b
        };
        let dest_dir = self
            .sftp
            .upload_dest_override
            .take()
            .unwrap_or_else(|| self.cur_sftp().pane(dest_side).remote_path.clone());
        // A move within one SSH session is a rename: instant, atomic, and
        // it keeps ownership, permissions and timestamps that a copy plus
        // delete would rebuild. Known synchronously, so the task can be
        // shaped for it: the rename path finishes without a queue, and
        // both panes need refreshing when it does.
        let try_rename = move_source && src_client.shares_session_with(&dst_client);
        let src_refresh = self.cur_sftp().pane(from).remote_path.clone();
        let dst_refresh = self.cur_sftp().pane(dest_side).remote_path.clone();
        let build = Task::perform(
            async move {
                let basename = src_path
                    .rsplit('/')
                    .find(|s| !s.is_empty())
                    .unwrap_or(&src_path)
                    .to_string();
                if move_source && same_host {
                    // Moving into the folder the item already sits in is a
                    // no-op, and the unique-name step below would quietly
                    // turn it into a RENAME instead: the name is taken by
                    // the source itself, so the copy lands beside it as
                    // "x (1)" and the original is then removed. Refuse,
                    // rather than silently renaming someone's file
                    // (issue #115).
                    if destinations_are_one_directory(
                        &src_client,
                        &parent_path(&src_path),
                        &dst_client,
                        &dest_dir,
                    )
                    .await
                    {
                        return Err(crate::i18n::t("sftp_move_same_directory").to_string());
                    }
                }
                // Pick a non-colliding name on the destination so a
                // transfer never silently clobbers an existing file with
                // the same name.
                let unique = unique_name_in_remote_dir(&dst_client, &dest_dir, &basename).await?;
                let target = remote_join(&dest_dir, &unique);
                if same_host
                    && relay_target_is_inside_source(
                        &resolved_path(&src_client, &src_path).await,
                        &resolved_path(&dst_client, &target).await,
                    )
                {
                    return Err(crate::i18n::t("sftp_relay_into_itself").to_string());
                }
                if try_rename {
                    // Falls through to copy plus delete on failure, which
                    // is what a cross-filesystem move on one host needs
                    // (`/home` and `/mnt/data` are one session but two
                    // devices, and rename cannot span them).
                    match src_client.rename(&src_path, &target).await {
                        Ok(()) => return Ok(None),
                        Err(e) => tracing::debug!(
                            "move: rename {src_path} -> {target} failed, \
                             falling back to copy and delete: {e}"
                        ),
                    }
                }
                let mut queue = std::collections::VecDeque::new();
                if is_dir {
                    queue.push_back(crate::state::TransferItem {
                        src: src_path.clone(),
                        dst: target.clone(),
                        is_dir: true,
                        size: None,
                    });
                    // Walk the SOURCE remote tree, mapping each entry onto
                    // a destination POSIX path under the target root.
                    walk_remote_for_relay(&src_client, &src_path, &target, &mut queue).await?;
                } else {
                    queue.push_back(crate::state::TransferItem {
                        src: src_path.clone(),
                        dst: target,
                        is_dir: false,
                        size: None,
                    });
                }
                let label = if is_dir { unique } else { basename };
                // Relay runs at concurrency 1: one source client slot plus
                // the single dest client.
                let state = crate::state::TransferState::new(
                    crate::state::TransferKind::Relay,
                    label,
                    queue,
                    vec![src_client],
                    Some(dst_client),
                    Some(dest_side),
                    1,
                );
                Ok::<Option<crate::state::TransferState>, String>(Some(if move_source {
                    // The removal list is the queue itself: same paths,
                    // same walk, so it cannot describe anything that was
                    // not copied.
                    let sources: Vec<crate::state::TransferItem> =
                        state.queue.iter().cloned().collect();
                    state.moving(sources)
                } else {
                    state
                }))
            },
            move |result| match result {
                Ok(Some(state)) => Message::Sftp(SftpMessage::SftpTransferQueueReady(owner, state)),
                // Renamed: no queue ever existed, so the source pane is
                // refreshed here and the destination by the chain below.
                Ok(None) => {
                    Message::Sftp(SftpMessage::SftpNavigateRemote(from, src_refresh.clone()))
                }
                Err(e) => Message::Sftp(SftpMessage::SftpOpResult(from, e, true)),
            },
        );
        if try_rename {
            build.chain(Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(
                dest_side,
                dst_refresh,
            ))))
        } else {
            build
        }
    }
}
