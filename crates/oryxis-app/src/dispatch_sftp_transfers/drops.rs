//! Files dropped onto the SFTP surface from the OS.
//!
//! The drop arrives one path per event, so the paths are buffered and
//! flushed on a debounce; hover state is tracked alongside so the pane
//! can show where the drop will land.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use super::SftpSides;

impl Oryxis {
    pub(super) fn handle_sftp_drops(
        &mut self,
        message: SftpMessage,
        sides: SftpSides,
    ) -> Result<Task<Message>, SftpMessage> {
        let SftpSides { remote: remote_side, local: _local_side, owner: _ } = sides;
        match message {
            SftpMessage::SftpFileHovered => {
                self.sftp.drop_active = true;
                // The window-wide mirror the sidebar Files hint reads
                // (issue #167); `drop_active` only reaches the SFTP
                // surface's own outline.
                self.os_drop_hover = true;
            }
            SftpMessage::SftpFilesHoveredLeft => {
                self.sftp.drop_active = false;
                self.os_drop_hover = false;
            }
            SftpMessage::SftpFileDropped(path) => {
                self.os_drop_hover = false;
                // OS drops only land in a remote folder when the
                // hovered row is on the remote pane AND a folder.
                let target_folder = self
                    .sftp
                    .hovered_row
                    .as_ref()
                    .filter(|(s, _, is_dir)| *s == remote_side && *is_dir)
                    .map(|(_, p, _)| p.clone());
                self.sftp.drop_active = false;
                // Deliberately NOT gated on `drop_active`: a FileDropped
                // only ever arrives from a genuine OS drop, and requiring
                // the hover flag broke real gestures twice over. A
                // multi-file drop delivers one FileDropped per file, so
                // the first file consumed the flag and the rest were
                // silently ignored; and a missed/late FileHovered
                // (observed on Windows after a previous drop) killed the
                // whole next gesture. The flag now only powers the drop
                // highlight.
                if !self.sftp_surface_visible() {
                    // Not an SFTP drop at all: an SFTP tab exists (the
                    // owner gate above passed) but a terminal is what's
                    // on screen. Hand the file to the terminal drop
                    // router (#106) instead of swallowing it.
                    return Ok(self.buffer_terminal_drop(path));
                }
                let in_remote_pane =
                    target_folder.is_some() || self.is_cursor_over_remote_pane();
                if !in_remote_pane {
                    return Ok(Task::none());
                }
                if self.cur_sftp().pane(remote_side).client.is_none() {
                    self.sftp.pane_mut(remote_side).error = Some(crate::i18n::t("sftp_not_connected").to_string());
                    return Ok(Task::none());
                }
                // A multi-select drop arrives as one FileDropped per
                // file. Collect the burst and flush once, so it becomes
                // a single batch transfer instead of N transfers racing
                // for the queue UI. The first file of the gesture pins
                // the destination (folder row vs pane dir) for them all.
                if self.cur_sftp().pending_drops.is_empty() {
                    self.sftp.upload_dest_override = target_folder;
                    self.sftp.pending_drops.push(path);
                    return Ok(Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                150,
                            ))
                            .await;
                        },
                        |_| Message::Sftp(SftpMessage::SftpDropFlush),
                    ));
                }
                self.sftp.pending_drops.push(path);
            }
            SftpMessage::SftpDropFlush => {
                let mut paths = std::mem::take(&mut self.sftp.pending_drops);
                // The upload handlers below consume `upload_dest_override`
                // (set when the burst started) before falling back to the
                // pane's remote dir.
                return Ok(match paths.len() {
                    0 => Task::none(),
                    // A lone FOLDER keeps its own arm, which picks a
                    // non-colliding name instead of merging into an
                    // existing one. A lone FILE rides the batch like every
                    // other drop: `SftpUpload` creates no `TransferState`,
                    // so it moved with no progress strip and no cancel.
                    1 if paths[0].is_dir() => {
                        Task::done(Message::Sftp(SftpMessage::SftpUploadFolder(paths.remove(0))))
                    }
                    _ => Task::done(Message::Sftp(SftpMessage::SftpUploadBatch(paths))),
                });
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
