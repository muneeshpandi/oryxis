//! The queue runner: slots, the next item, completion, failure, cancel.
//!
//! One `TransferState` at a time per owner, with N concurrent slots over
//! a pool of clients. `SftpTransferNext` is the only place that decides
//! what runs, so back-pressure, retries and the finalize arm (where a
//! move removes its sources) all live together here.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::sftp_helpers::{
    do_download_item, do_local_duplicate_item, do_relay_item, do_upload_item, remove_moved_sources,
    transfer_item_label, TransferStepOutcome,
};
use crate::state::SftpPaneSide;
use super::SftpSides;

impl Oryxis {
    pub(super) fn handle_sftp_queue(
        &mut self,
        message: SftpMessage,
        sides: SftpSides,
    ) -> Result<Task<Message>, SftpMessage> {
        let SftpSides { remote: remote_side, local: local_side, owner } = sides;
        match message {
            SftpMessage::SftpTransferQueueReady(_, state) => {
                let slot_count = state.busy_slots.len().max(1);
                let verb_key = match state.kind {
                    crate::state::TransferKind::Upload => "sftp_log_uploading",
                    crate::state::TransferKind::Download => "sftp_log_downloading",
                    crate::state::TransferKind::DuplicateLocal => "sftp_log_duplicating",
                    crate::state::TransferKind::Relay => "sftp_log_relaying",
                };
                // The message log belongs to the dual-pane surface and is
                // the only thing here a sidebar owner has no place for:
                // pushing into it would file this transfer under whichever
                // SFTP tab happens to be live, which is a different host.
                if !self.is_sidebar_owner(owner) {
                    self.push_sftp_log(
                        crate::state::SftpLogLevel::Info,
                        format!(
                            "{} {} ({} {})",
                            crate::i18n::t(verb_key),
                            state.root_label,
                            state.total,
                            crate::i18n::t("sftp_log_items"),
                        ),
                    );
                }
                let Some(slot) = self.transfer_slot_mut(owner) else {
                    return Ok(Task::none());
                };
                // Fresh transfer: reset the per-file panel log + collapse it.
                slot.done_log.clear();
                slot.panel_open = false;
                // Live byte progress: total = sum of known item sizes (0 if
                // unknown, bar falls back to item counts). Use a *fresh*
                // counter rather than resetting the old one, so a lingering
                // worker from a previous/cancelled transfer (whose task may
                // still be draining) can't keep incrementing this transfer's
                // counter and spike the bar to 100% before its first byte.
                slot.bytes_total = state.queue.iter().filter_map(|i| i.size).sum();
                slot.bytes_done = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                slot.state = Some(state);
                // Kick off one Next per slot so the worker pool fills
                // up immediately. Each completion will dispatch its
                // own Next to keep the chain going.
                let initial: Vec<Task<Message>> = (0..slot_count)
                    .map(|_| Task::done(Message::Sftp(SftpMessage::SftpTransferNext(owner))))
                    .collect();
                return Ok(Task::batch(initial));
            }
            SftpMessage::SftpTransferNext(_) => {
                // Read before the slot borrow below, which runs to the end
                // of the arm.
                let temp_name = self.prefs.sftp_upload_temp_name;
                let Some(book) = self.transfer_slot_mut(owner) else {
                    return Ok(Task::none());
                };
                // Taken BEFORE the borrow below, which runs to the end of
                // the arm and comes out of the same slot. The counter is
                // shared with the worker either way.
                let bytes_done = book.bytes_done.clone();
                let Some(transfer) = book.state.as_mut() else {
                    return Ok(Task::none());
                };
                if transfer.paused {
                    // Modal is up, workers idle until the user picks
                    // an action. Resolve will re-dispatch Next for
                    // each slot then.
                    return Ok(Task::none());
                }
                if transfer.dir_slot.is_some() {
                    // A directory item is in flight. It's an ordering
                    // barrier (see `TransferState::dir_slot`): nothing
                    // queued behind it may start until it exists. Its
                    // ItemDone refills the pool.
                    return Ok(Task::none());
                }
                if transfer.queue.front().is_some_and(|i| i.is_dir)
                    && transfer.busy_slots.iter().any(|b| *b)
                {
                    // Next up is a directory: drain the in-flight items
                    // first so everything queued before it (its own
                    // parent dir included) has finished. The pending
                    // ItemDones re-dispatch Next.
                    return Ok(Task::none());
                }
                let Some(slot) = transfer
                    .busy_slots
                    .iter()
                    .position(|b| !b)
                    .map(|i| i as u8)
                else {
                    // All slots busy, Next dispatch by ItemDone is
                    // ahead of an already-busy slot. Drop it; the
                    // next ItemDone will free a slot.
                    return Ok(Task::none());
                };
                let Some(item) = transfer.queue.pop_front() else {
                    // Queue exhausted. If every slot is idle, finalize
                    // and refresh; otherwise wait for in-flight slots
                    // to drain.
                    if transfer.busy_slots.iter().all(|b| !b) {
                        let kind = transfer.kind;
                        // Relay refreshes its actual destination pane,
                        // which may be the left pane (right-to-left relay),
                        // not the canonical remote (`remote_side`).
                        let relay_dest = transfer.dest_side;
                        let root_label = transfer.root_label.clone();
                        // A MOVE removes its sources here and nowhere
                        // else. Reaching this arm is the proof the copy
                        // succeeded: every item was popped, every worker
                        // reported done, and any single failure would
                        // have cleared `transfer` from the error arm long
                        // before the queue could drain (issue #115).
                        let move_sources = transfer.move_sources.take();
                        let move_client = transfer.clients.first().cloned();
                        if let Some(slot) = self.transfer_slot_mut(owner) {
                            slot.state = None;
                        }
                        // Both owners pass through here, so the notice
                        // goes BEFORE the sidebar's early return below.
                        self.notify_transfer_finished(
                            crate::i18n::t("transfer_notify_done"),
                            &root_label,
                        );
                        if self.is_sidebar_owner(owner) {
                            // A sidebar transfer has one thing to refresh
                            // (its own listing) and none of the dual-pane
                            // vocabulary below: no local pane to re-read,
                            // no relay destination, and moves never start
                            // from here.
                            return Ok(self
                                .sidebar_transfer_refresh(owner)
                                .unwrap_or_else(Task::none));
                        }
                        self.push_sftp_log(
                            crate::state::SftpLogLevel::Ok,
                            format!("{} {}", crate::i18n::t("sftp_log_transfer_done"), root_label),
                        );
                        if let (Some(sources), Some(client)) = (move_sources, move_client) {
                            let src_side = if relay_dest == Some(SftpPaneSide::Left) {
                                SftpPaneSide::Right
                            } else {
                                SftpPaneSide::Left
                            };
                            let dst_side = relay_dest.unwrap_or(remote_side);
                            let src_path = self.cur_sftp().pane(src_side).remote_path.clone();
                            let dst_path = self.cur_sftp().pane(dst_side).remote_path.clone();
                            return Ok(Task::perform(
                                remove_moved_sources(client, sources),
                                move |r| match r {
                                    // Both panes changed: the source lost
                                    // the tree, the destination gained it.
                                    Ok(_) => Message::Sftp(SftpMessage::SftpNavigateRemote(
                                        src_side,
                                        src_path.clone(),
                                    )),
                                    // The copy is safe on the other host;
                                    // only the cleanup failed, so say that
                                    // and leave the source alone.
                                    Err(e) => Message::Sftp(SftpMessage::SftpOpResult(
                                        src_side, e, true,
                                    )),
                                },
                            )
                            .chain(Task::done(Message::Sftp(
                                SftpMessage::SftpNavigateRemote(dst_side, dst_path),
                            ))));
                        }
                        return Ok(match kind {
                            crate::state::TransferKind::Relay => {
                                let dst = relay_dest.unwrap_or(remote_side);
                                Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(
                                    dst,
                                    self.cur_sftp().pane(dst).remote_path.clone(),
                                )))
                            }
                            crate::state::TransferKind::Upload => Task::done(
                                Message::Sftp(SftpMessage::SftpNavigateRemote(
                                    remote_side,
                                    self.cur_sftp().pane(remote_side).remote_path.clone(),
                                )),
                            ),
                            crate::state::TransferKind::Download
                            | crate::state::TransferKind::DuplicateLocal => {
                                self.refresh_sftp_local(local_side);
                                Task::none()
                            }
                        });
                    }
                    return Ok(Task::none());
                };
                transfer.busy_slots[slot as usize] = true;
                if item.is_dir {
                    transfer.dir_slot = Some(slot);
                }
                transfer.current = Some(transfer_item_label(&item));
                let kind = transfer.kind;
                let overwrite_default = transfer.overwrite_default;
                let multi = transfer.total > 1;
                // Shared live-byte counter the worker increments as chunks
                // move; the tick subscription polls it for the bar.
                match kind {
                    crate::state::TransferKind::Upload => {
                        let Some(client) = transfer.clients.get(slot as usize).cloned() else {
                            return Ok(Task::done(Message::Sftp(SftpMessage::SftpTransferError(
                                owner,
                                "transfer: slot has no client".into(),
                                slot,
                            ))));
                        };
                        return Ok(Task::perform(
                            do_upload_item(
                                client,
                                item,
                                overwrite_default,
                                multi,
                                Some(bytes_done),
                                temp_name,
                            ),
                            move |r| match r {
                                Ok(TransferStepOutcome::Done) => {
                                    Message::Sftp(SftpMessage::SftpTransferItemDone(owner, slot))
                                }
                                Ok(TransferStepOutcome::Conflict { prompt, item }) => {
                                    Message::Sftp(SftpMessage::SftpTransferConflict(owner, prompt, item, slot))
                                }
                                Err(e) => Message::Sftp(SftpMessage::SftpTransferError(owner, e, slot)),
                            },
                        ));
                    }
                    crate::state::TransferKind::Download => {
                        let Some(client) = transfer.clients.get(slot as usize).cloned() else {
                            return Ok(Task::done(Message::Sftp(SftpMessage::SftpTransferError(
                                owner,
                                "transfer: slot has no client".into(),
                                slot,
                            ))));
                        };
                        return Ok(Task::perform(
                            do_download_item(client, item, overwrite_default, multi, Some(bytes_done)),
                            move |r| match r {
                                Ok(TransferStepOutcome::Done) => {
                                    Message::Sftp(SftpMessage::SftpTransferItemDone(owner, slot))
                                }
                                Ok(TransferStepOutcome::Conflict { prompt, item }) => {
                                    Message::Sftp(SftpMessage::SftpTransferConflict(owner, prompt, item, slot))
                                }
                                Err(e) => Message::Sftp(SftpMessage::SftpTransferError(owner, e, slot)),
                            },
                        ));
                    }
                    crate::state::TransferKind::Relay => {
                        // Source client for the slot, plus the single
                        // dest-host client (relay runs at concurrency 1).
                        let Some(src_client) = transfer.clients.get(slot as usize).cloned() else {
                            return Ok(Task::done(Message::Sftp(SftpMessage::SftpTransferError(
                                owner,
                                "transfer: slot has no client".into(),
                                slot,
                            ))));
                        };
                        let Some(dst_client) = transfer.dest_client.clone() else {
                            return Ok(Task::done(Message::Sftp(SftpMessage::SftpTransferError(
                                owner,
                                "relay: missing destination client".into(),
                                slot,
                            ))));
                        };
                        // A move verifies every file landed at the right
                        // size before anything is removed later; a copy
                        // does not pay for that round trip.
                        let verify = transfer.move_sources.is_some();
                        return Ok(Task::perform(
                            do_relay_item(src_client, dst_client, item, Some(bytes_done), verify),
                            move |r| match r {
                                Ok(()) => Message::Sftp(SftpMessage::SftpTransferItemDone(owner, slot)),
                                Err(e) => Message::Sftp(SftpMessage::SftpTransferError(owner, e, slot)),
                            },
                        ));
                    }
                    crate::state::TransferKind::DuplicateLocal => {
                        // Sync, no need for an async task.
                        return Ok(match do_local_duplicate_item(&item) {
                            Ok(()) => Task::done(Message::Sftp(SftpMessage::SftpTransferItemDone(owner, slot))),
                            Err(e) => Task::done(Message::Sftp(SftpMessage::SftpTransferError(owner, e, slot))),
                        });
                    }
                }
            }
            SftpMessage::SftpTransferItemDone(_, slot) => {
                // Record the finished item's label for the per-file panel.
                // `current` is the label set when this item was dispatched
                // (exact at the relay's concurrency of 1; an approximation
                // at higher concurrency, good enough for a status list).
                // NOT `slot`: this arm already has one, the worker index.
                let Some(book) = self.transfer_slot_mut(owner) else {
                    return Ok(Task::none());
                };
                let finished = book.state.as_ref().and_then(|t| t.current.clone());
                let mut refill = 1usize;
                if let Some(transfer) = book.state.as_mut() {
                    transfer.completed += 1;
                    transfer.current = None;
                    if (slot as usize) < transfer.busy_slots.len() {
                        transfer.busy_slots[slot as usize] = false;
                    }
                    if transfer.dir_slot == Some(slot) {
                        // Barrier lifted: the dir exists now, so refill
                        // the whole pool (Next dispatched nothing while
                        // it was in flight; extra Nexts drop harmlessly
                        // on the all-busy guard).
                        transfer.dir_slot = None;
                        refill = transfer.busy_slots.len().max(1);
                    }
                }
                if let Some(label) = finished {
                    book.done_log.push(label);
                }
                let next: Vec<Task<Message>> = (0..refill)
                    .map(|_| Task::done(Message::Sftp(SftpMessage::SftpTransferNext(owner))))
                    .collect();
                return Ok(Task::batch(next));
            }
            SftpMessage::SftpTransferError(_, e, _slot) => {
                // Errors abort the whole transfer, the in-flight item
                // failed and we don't try to be clever about retrying
                // siblings (a network blip is likely to nuke them all).
                let (kind, relay_dest) = match self.transfer_slot_mut(owner) {
                    Some(slot) => {
                        let k = slot.state.as_ref().map(|t| t.kind);
                        let d = slot.state.as_ref().and_then(|t| t.dest_side);
                        slot.state = None;
                        (k, d)
                    }
                    None => return Ok(Task::none()),
                };
                // The message has to OUTLIVE the toast that used to carry
                // it: the reported 3 GB download died while the user was
                // looking elsewhere, and a transient toast is exactly how
                // "everything stopped" became all they knew. Same
                // reasoning gives a failure the OS notice a completion
                // gets: away from the window, "it stopped" is the half
                // that matters most.
                self.notify_transfer_finished(crate::i18n::t("transfer_notify_failed"), &e);
                match kind {
                    Some(crate::state::TransferKind::DuplicateLocal) => {
                        self.transfer_set_error(owner, local_side, e);
                        self.refresh_sftp_local(local_side);
                    }
                    Some(crate::state::TransferKind::Relay) => {
                        let dst = relay_dest.unwrap_or(remote_side);
                        self.transfer_set_error(owner, dst, e);
                    }
                    _ => {
                        self.transfer_set_error(owner, remote_side, e);
                    }
                }
            }
            SftpMessage::SftpCancelTransfer => {
                let (kind, relay_dest) = match self.transfer_slot_mut(owner) {
                    Some(slot) => {
                        let k = slot.state.as_ref().map(|t| t.kind);
                        let d = slot.state.as_ref().and_then(|t| t.dest_side);
                        slot.state = None;
                        (k, d)
                    }
                    None => return Ok(Task::none()),
                };
                if self.is_sidebar_owner(owner) {
                    return Ok(self.sidebar_transfer_refresh(owner).unwrap_or_else(Task::none));
                }
                // The in-flight item can't be aborted mid-byte (russh-sftp
                // doesn't expose a cancel token), but no further items
                // will run, and the user can refresh to see the partial
                // result.
                match kind {
                    Some(crate::state::TransferKind::Relay) => {
                        let dst = relay_dest.unwrap_or(remote_side);
                        return Ok(Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(
                            dst,
                            self.cur_sftp().pane(dst).remote_path.clone(),
                        ))));
                    }
                    Some(crate::state::TransferKind::Upload) => {
                        return Ok(Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(
                            remote_side,
                            self.cur_sftp().pane(remote_side).remote_path.clone(),
                        ))));
                    }
                    Some(_) => {
                        self.refresh_sftp_local(local_side);
                    }
                    None => {}
                }
            }
            SftpMessage::SftpTransferTick => {}
            SftpMessage::SftpToggleTransferPanel => {
                if let Some(slot) = self.transfer_slot_mut(owner) {
                    slot.panel_open = !slot.panel_open;
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// A queue that drained (or died) while the user was elsewhere.
    /// In front of the window the progress panel already says it, so
    /// the toast is only for the case where the notice was owed and
    /// the OS refused to carry it.
    fn notify_transfer_finished(&mut self, title: &str, body: &str) {
        if !self.cur_focused() {
            self.notify_away(title, body, Some(format!("{title}: {body}")));
        }
    }
}
