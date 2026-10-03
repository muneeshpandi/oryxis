//! Starting a transfer for ONE item: download (including the
//! "download to..." destination pick) and local duplicate.
//!
//! Neither direction has a bespoke single-file path any more: both
//! delegate to the batched queue runner, which is the only thing that
//! draws the progress strip. What is left here is the routing and the
//! local duplicate, which never touches the network. Nothing here loops.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::sftp_helpers::{
    parent_path, remote_cp, remote_join, unique_name_in_local_dir, unique_name_in_remote_dir,
};
use super::SftpSides;

impl Oryxis {
    pub(super) fn handle_sftp_single(
        &mut self,
        message: SftpMessage,
        // Uniform with the other slices, and unused here: what is left
        // in this one either delegates or is side-addressed by the
        // message itself.
        _sides: SftpSides,
    ) -> Result<Task<Message>, SftpMessage> {
        match message {
            SftpMessage::SftpDownload(remote_path) => {
                // A single file goes through the batched queue runner like
                // every other download, so it gets the progress strip, the
                // per-file panel and a cancel button. This arm used to run
                // `download_to` inline: correct, but a 49 MB file (field
                // report) moved with nothing at all on screen while the
                // upload side had shown a bar since v0.4.0. The queue also
                // owns the space check, the unsafe-name rule and the
                // overwrite prompt, so nothing is lost by delegating.
                //
                // The destination ask lives in the batch arm alone. The
                // "Download to..." wrapper still works: its pick sets
                // `download_dest_override`, re-dispatches this alias, and
                // the batch's own ask declines because the override is not
                // consumed until the transfer is built.
                self.sftp.row_menu = None;
                Ok(Task::done(Message::Sftp(SftpMessage::SftpDownloadBatch(vec![(
                    remote_path,
                    false,
                )]))))
            }
            SftpMessage::SftpDownloadTo(then) => {
                self.sftp.row_menu = None;
                // Explicit ask, so an override left over from a drop onto
                // a folder must not short-circuit it.
                self.sftp.download_dest_override = None;
                Ok(self.sftp_pick_download_dir(*then))
            }
            SftpMessage::SftpDownloadDestPicked(dir, then) => {
                let Some(dir) = dir else {
                    // Cancelled: nothing was touched, in particular the
                    // override stays unset so the next download still asks.
                    return Ok(Task::none());
                };
                self.sftp.download_dest_override = Some(dir);
                Ok(Task::done(Message::Sftp(*then)))
            }
            SftpMessage::SftpDuplicate(side, path) => {
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
                        let dest = parent.join(&unique);
                        // The copy can be multi-GB; run it off the event
                        // loop instead of freezing update() for the
                        // duration, mirroring the remote branch below.
                        Ok(Task::perform(
                            tokio::task::spawn_blocking(move || std::fs::copy(&src, &dest)),
                            move |res| match res {
                                Ok(Ok(_)) => Message::Sftp(SftpMessage::SftpRefreshLocal(side)),
                                Ok(Err(e)) => Message::Sftp(SftpMessage::SftpOpResult(side, format!("copy: {e}"), true)),
                                Err(e) => Message::Sftp(SftpMessage::SftpOpResult(side, format!("copy: {e}"), true)),
                            },
                        ))
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
                        Ok(Task::perform(
                            async move {
                                let unique =
                                    unique_name_in_remote_dir(&client, &parent, &basename)
                                        .await?;
                                let dest = remote_join(&parent, &unique);
                                // `cp -- src dst`, same exec channel trick
                                // we used for `rm -rf`. Using -- prevents
                                // dashes in names from being parsed as flags.
                                remote_cp(&client, &src, &dest, false).await?;
                                Ok::<String, String>(reload)
                            },
                            move |result| match result {
                                Ok(reload) => Message::Sftp(SftpMessage::SftpNavigateRemote(side, reload)),
                                Err(e) => Message::Sftp(SftpMessage::SftpOpResult(side, e, true)),
                            },
                        ))
                }
            }
            // Every arm above returns, so the match IS the return
            // value here rather than a statement before one.
            m => Err(m),
        }
    }
}
