//! Answering "that name is taken".
//!
//! Raised from either direction (upload and download both ask now), and
//! answered with Replace / Replace if different / Duplicate / Cancel,
//! optionally sticky for the rest of a batch. The sticky answer is what
//! makes this a state machine rather than a dialog: the runner consults
//! it on every later item instead of asking again.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::sftp_helpers::{apply_overwrite_for_download_item, apply_overwrite_for_item};
use super::SftpSides;

impl Oryxis {
    pub(super) fn handle_sftp_conflict(
        &mut self,
        message: SftpMessage,
        sides: SftpSides,
    ) -> Result<Task<Message>, SftpMessage> {
        let SftpSides { remote: remote_side, local: _, owner } = sides;
        match message {
            SftpMessage::SftpToggleApplyToAll => {
                if let Some(p) = self.sftp.overwrite_prompt.as_mut() {
                    p.apply_to_all = !p.apply_to_all;
                }
            }
            SftpMessage::SftpResolveOverwrite(action) => {
                let Some(prompt) = self.sftp.overwrite_prompt.take() else {
                    return Ok(Task::none());
                };
                // Terminal / sidebar Files drop upload conflicts are
                // intercepted earlier in `handle_sftp_transfers` (they
                // must resolve even with no SFTP tab open); a prompt
                // reaching here without a queue behind it is a stale
                // drop conflict, so decline it instead of guessing.
                if prompt.drop_upload_pane.is_some() {
                    return Ok(Task::none());
                }
                let apply_to_all = prompt.apply_to_all;
                let temp_name = self.prefs.sftp_upload_temp_name;
                let downloading =
                    prompt.direction == crate::state::OverwriteDirection::Download;
                // The dual-pane navigation client is only a fallback
                // here: a queue transfer carries its own per-slot
                // clients, and a sidebar-owned queue may run while the
                // dual-pane surface has no client at all. Requiring it
                // up front used to swallow the answer and park the
                // queue forever.
                let pane_client = self.cur_sftp().pane(remote_side).client.clone();
                // Pull the item the runner parked when it raised this
                // prompt. Every conflict comes from a queue now, single
                // file included, so this is the only flow: apply the
                // answer to that item, sticky for the rest if asked.
                let (pending_item, pending_slot, slot_count) =
                    self.transfer_slot_mut(owner).and_then(|s| s.state.as_mut()).map_or(
                        (None, None, 0usize),
                        |t| {
                            // Resume is deliberately never sticky, even
                            // with the box ticked: what the engine can
                            // check is that this destination's last block
                            // matches THIS source's, which says nothing
                            // about the next pair. A sticky "continue"
                            // would be guessing at whether to splice two
                            // files together, on files it has not seen.
                            if apply_to_all
                                && !matches!(action, crate::state::OverwriteAction::Resume)
                            {
                                t.overwrite_default = Some(action);
                            }
                            // Resume the worker pool, set paused false
                            // so the resume Next dispatches succeed.
                            t.paused = false;
                            (
                                t.pending_conflict_item.take(),
                                t.pending_conflict_slot.take(),
                                t.busy_slots.len(),
                            )
                        },
                    );
                let Some(item) = pending_item else {
                    // The prompt outlived its transfer: cancelled, or
                    // its tab closed, while the modal was up. Every
                    // prompt is raised by the queue runner now, so
                    // there is nothing else this answer could belong
                    // to; acting on it would write to a destination no
                    // transfer is heading for.
                    return Ok(Task::none());
                };
                if matches!(action, crate::state::OverwriteAction::Cancel) {
                    // Cancel skips this item; with apply-to-all it
                    // also drops the rest of the queue so the user
                    // doesn't keep getting prompted.
                    if apply_to_all
                        && let Some(t) = self.transfer_slot_mut(owner).and_then(|s| s.state.as_mut())
                    {
                        t.queue.clear();
                    }
                    let slot = pending_slot.unwrap_or(0);
                    // Free slot bookkeeping handled by ItemDone.
                    // Also kick a Next per other slot so the rest
                    // of the workers resume from pause.
                    let mut tasks =
                        vec![Task::done(Message::Sftp(SftpMessage::SftpTransferItemDone(owner, slot)))];
                    for _ in 1..slot_count {
                        tasks.push(Task::done(Message::Sftp(SftpMessage::SftpTransferNext(owner))));
                    }
                    return Ok(Task::batch(tasks));
                }
                let slot = pending_slot.unwrap_or(0);
                // Use the slot's own SFTP client for the apply
                // step; the navigation client is only a fallback
                // for a somehow-stale slot index. With neither,
                // fail the item through the transfer's own error
                // path (visible on the strip and the tab badge),
                // never a pane error the owner may not render.
                let Some(client) = self
                    .transfer_slot_mut(owner)
                    .and_then(|s| s.state.as_ref())
                    .and_then(|t| t.clients.get(slot as usize).cloned())
                    .or(pane_client)
                else {
                    return Ok(Task::done(Message::Sftp(SftpMessage::SftpTransferError(
                        owner,
                        crate::i18n::t("sftp_not_connected").to_string(),
                        slot,
                    ))));
                };
                if let Some(t) = self.transfer_slot_mut(owner).and_then(|s| s.state.as_mut())
                    && (slot as usize) < t.busy_slots.len()
                {
                    t.busy_slots[slot as usize] = true;
                }
                // The apply step writes to whichever side the prompt
                // came from: an upload lands on the remote host, a
                // download on the local filesystem. Same continuation
                // either way (it captures only Copy state, so it is
                // itself Copy and both arms can use it).
                let bytes_done = self
                    .transfer_slot_mut(owner)
                    .map(|s| s.bytes_done.clone());
                let done = move |r: Result<(), String>| match r {
                    Ok(()) => Message::Sftp(SftpMessage::SftpTransferItemDone(owner, slot)),
                    Err(e) => Message::Sftp(SftpMessage::SftpTransferError(owner, e, slot)),
                };
                let mut tasks = vec![if downloading {
                    Task::perform(
                        apply_overwrite_for_download_item(client, item, action, bytes_done),
                        done,
                    )
                } else {
                    Task::perform(
                        apply_overwrite_for_item(client, item, action, temp_name, bytes_done),
                        done,
                    )
                }];
                // Resume the other slots that exited on pause.
                for _ in 1..slot_count {
                    tasks.push(Task::done(Message::Sftp(SftpMessage::SftpTransferNext(owner))));
                }
                return Ok(Task::batch(tasks));
            }
            SftpMessage::SftpTransferConflict(_, mut prompt, item, slot) => {
                prompt.owner = Some(owner);
                // Park the popped item alongside the prompt so the
                // resolve handler knows which destination the user is
                // about to act on. The queue stays stalled here until
                // the modal is answered.
                if let Some(transfer) = self.transfer_slot_mut(owner).and_then(|s| s.state.as_mut()) {
                    transfer.pending_conflict_item = Some(item);
                    transfer.pending_conflict_slot = Some(slot);
                    transfer.paused = true;
                    if (slot as usize) < transfer.busy_slots.len() {
                        transfer.busy_slots[slot as usize] = false;
                    }
                }
                self.sftp.overwrite_prompt = Some(prompt);
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
