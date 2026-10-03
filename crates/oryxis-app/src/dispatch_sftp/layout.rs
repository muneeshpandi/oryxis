//! Pane chrome arms split out of `dispatch_sftp`: the actions / drives
//! / filter popovers, column toggles / resize / auto-fit / drag, the
//! transfer-log toggle and the split / log resize drags. Called from
//! `handle_sftp`.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::state::SftpPaneSide;

impl Oryxis {
    /// Auto-fit `col` in `side`'s pane to the widest value across every row
    /// (issue #45). Measures through the renderer's font system, sets the new
    /// width (clamped), then re-seeds + persists the column template.
    fn autofit_sftp_column(&mut self, side: SftpPaneSide, col: crate::state::SftpColumn) {
        let target = {
            let pane = self.cur_sftp().pane(side);
            crate::views::sftp::autofit_column_width(
                pane.is_remote,
                &pane.remote_entries,
                &pane.local_entries,
                col,
            )
        };
        self.sftp.pane_mut(side).columns.width.set_autofit(col, target);
        self.sftp_chrome.columns_template = self.cur_sftp().pane(side).columns.clone();
        self.persist_sftp_columns();
    }

    pub(super) fn handle_sftp_layout(
        &mut self,
        message: SftpMessage,
    ) -> Result<Task<Message>, SftpMessage> {
        match message {
            SftpMessage::SftpToggleActions(side) => {
                let now = !self.cur_sftp().pane(side).actions_open;
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.left.drives_open = false;
                self.sftp.left.filter_open = false;
                self.sftp.right.filter_open = false;
                self.sftp.pane_mut(side).actions_open = now;
            }
            SftpMessage::SftpToggleDrives(side) => {
                let now = !self.cur_sftp().pane(side).drives_open;
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.left.drives_open = false;
                self.sftp.right.drives_open = false;
                self.sftp.pane_mut(side).drives_open = now;
            }
            SftpMessage::SftpCloseMenus => {
                self.sftp.close_menus();
            }
            SftpMessage::SftpToggleColumn(side, col) => {
                // Per-pane toggle; the actions menu stays open so the user can
                // flip several columns in one pass. The edited pane becomes the
                // new persisted template seed.
                self.sftp.pane_mut(side).columns.toggle(col);
                self.sftp_chrome.columns_template = self.cur_sftp().pane(side).columns.clone();
                self.persist_sftp_columns();
            }
            SftpMessage::SftpColResizeStart(side, col) => {
                let start_w = self.cur_sftp().pane(side).columns.width.get(col);
                self.sftp_chrome.col_resize = Some((side, col, self.cur_mouse().x, start_w));
                self.sftp.close_menus();
            }
            SftpMessage::SftpColAutoFit(side, col) => {
                self.autofit_sftp_column(side, col);
            }
            SftpMessage::SftpColDragStart(side, col) => {
                self.sftp_chrome.col_drag = Some(crate::state::SftpColDrag {
                    side,
                    col,
                    press_x: self.cur_mouse().x,
                    active: false,
                });
            }
            SftpMessage::SftpColHovered(side, col) => {
                self.sftp_chrome.hovered_col = Some((side, col));
            }
            SftpMessage::SftpColUnhovered(side, col) => {
                self.sftp_chrome.leave_col((side, col));
            }
            SftpMessage::SftpToggleFilterSearch(side) => {
                let now = !self.cur_sftp().pane(side).filter_open;
                self.sftp.close_menus();
                self.sftp.pane_mut(side).filter_open = now;
                if now {
                    // Focus the popover input so the user can type immediately.
                    let id = match side {
                        SftpPaneSide::Left => "sftp-filter-pop-left",
                        SftpPaneSide::Right => "sftp-filter-pop-right",
                    };
                    return Ok(crate::widgets::focus_input(iced::widget::Id::new(id)));
                }
            }
            SftpMessage::SftpToggleLog => {
                self.sftp.log_open = !self.cur_sftp().log_open;
            }
            SftpMessage::SftpSplitResizeStart => {
                // Capture the cursor x and current ratio; the MouseMoved
                // handler computes the delta against these.
                self.sftp_chrome.split_drag = Some((self.cur_mouse().x, self.sftp_chrome.split_ratio));
            }
            SftpMessage::SftpLogResizeStart => {
                // Capture the cursor y and current log height; the MouseMoved
                // handler computes the delta against these.
                self.sftp_chrome.log_drag = Some((self.cur_mouse().y, self.cur_sftp().log_height));
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
