//! Windows: opening one, moving tabs between them, closing one.
//!
//! The model is in `window_ctx`: every tab lives in `Oryxis::tabs`, a
//! window only lists which ones it shows. So nothing here touches a
//! session. Moving a tab (terminal or SFTP) is moving its strip ref from
//! one list to another.
//!
//! No window is special to the user: each has its own strip and its own
//! screen (the hosts list, a vault view, a tab), any of them can be
//! closed, and the app ends with the last one. Closing a window closes
//! its tabs, the way closing them one by one would; pinned tabs move to
//! another window instead, because a pin is a tab the user asked to
//! keep.
//!
//! Each function runs with the SOURCE window's values in the per-window
//! fields (`tab_order`, `active_tab`): that is the window the gesture
//! came from.

use iced::{window, Point, Task};
use uuid::Uuid;

use crate::app::{Message, Oryxis, SftpMessage, TabsMessage};
use crate::state::{TabRef, View};
use crate::window_ctx::ExtraWindow;

/// How far past an edge the cursor has to be before a dragged tab or
/// card leaves the window. The tab strip sits on the top edge, so a
/// reorder that drifts a few pixels over it must not count.
const TEAR_OFF_MARGIN: f32 = 24.0;

/// Where the new window's corner lands relative to the cursor that
/// dropped the tab: the chip ends up roughly under the pointer.
const DROP_OFFSET: Point = Point::new(60.0, 18.0);

impl Oryxis {
    /// Register a new window showing `order` on `view` and return the
    /// task that opens it. The id is known at once.
    fn open_extra_window(
        &mut self,
        order: Vec<TabRef>,
        view: View,
        at: Option<Point>,
    ) -> (window::Id, Task<Message>) {
        let mut settings = crate::app::WINDOW_SETTINGS
            .get()
            .cloned()
            .unwrap_or_default();
        // The size of the window the gesture came from, so a terminal
        // keeps its grid; never its maximized or fullscreen state.
        settings.size = self.window_size;
        settings.maximized = false;
        settings.fullscreen = false;
        settings.position = match at {
            Some(p) => window::Position::Specific(p),
            None => window::Position::Default,
        };
        let (id, open) = window::open(settings);
        self.extra_windows
            .insert(id, ExtraWindow::new(order, view, self.window_size));
        (id, open.discard())
    }

    /// The strip ref of the tab with strip id `id`, terminal or SFTP,
    /// when the current strip holds it.
    fn strip_ref(&self, id: Uuid) -> Option<TabRef> {
        self.tab_order
            .iter()
            .copied()
            .find(|r| !matches!(r, TabRef::Panel(_)) && r.strip_id() == id)
    }

    /// After a tab left the current strip from slot `slot`: show its
    /// neighbour, through the ordinary select of its kind, or the hosts
    /// screen with no tab next to it.
    fn select_after_tab_left(&mut self, slot: usize) -> Task<Message> {
        let next = self
            .tab_order
            .get(slot)
            .or_else(|| slot.checked_sub(1).and_then(|p| self.tab_order.get(p)))
            .copied();
        match next {
            Some(TabRef::Terminal(id)) => {
                if let Some(i) = self.tabs.iter().position(|t| t._id == id) {
                    self.active_view = View::Terminal;
                    return self.handle_select_tab(i);
                }
            }
            Some(TabRef::Sftp(id)) => {
                if let Some(i) = self.sftp_tabs.iter().position(|t| t.id == id) {
                    return self.update(Message::Sftp(SftpMessage::SelectSftpTab(i)));
                }
            }
            _ => {}
        }
        self.active_tab = None;
        self.active_view = View::Dashboard;
        Task::none()
    }

    /// Take a tab's ref out of the current strip, with the task that
    /// shows its neighbour. `None` when the strip does not hold it.
    ///
    /// The live SFTP buffer belongs to the window whose values are in
    /// the fields, so a surface that owns it goes home to its own slot
    /// first: it travels parked, and the window it lands in hoists it.
    fn release_tab_ref(&mut self, id: Uuid) -> Option<(TabRef, Task<Message>)> {
        let r = self.strip_ref(id)?;
        let slot = self.tab_order.iter().position(|x| *x == r)?;
        let was_active = match r {
            TabRef::Terminal(_) => {
                let idx = self.tabs.iter().position(|t| t._id == id)?;
                if self.hybrid_sftp_owner == Some(id) {
                    self.park_hybrid_sftp();
                }
                self.active_tab == Some(idx) && self.active_view != View::Sftp
            }
            TabRef::Sftp(_) => {
                let idx = self.sftp_tabs.iter().position(|t| t.id == id)?;
                let owned = self.active_sftp == Some(idx) && self.hybrid_sftp_owner.is_none();
                if owned {
                    self.sftp_click_gen = self.sftp_click_gen.wrapping_add(1);
                    self.sftp_tabs[idx].state = std::mem::take(&mut self.sftp);
                    self.active_sftp = None;
                }
                owned && self.active_view == View::Sftp
            }
            TabRef::Panel(_) => return None,
        };
        self.tab_order.remove(slot);
        let select = if was_active {
            self.select_after_tab_left(slot)
        } else {
            Task::none()
        };
        Some((r, select))
    }

    /// Whether moving the tab `id` into a window of its own makes
    /// sense: this strip holds something else, or this is the only
    /// window (the tab leaves and the hosts screen stays behind). A
    /// window that is one of several and holds only that tab already IS
    /// its window.
    fn can_detach(&self, id: Uuid) -> bool {
        self.tab_order.iter().any(|r| r.strip_id() != id) || self.window_count() == 1
    }

    /// Move a tab (terminal or SFTP, by strip id) out of the current
    /// window into a new one, opened at `at` (screen coordinates) when
    /// given.
    pub(super) fn detach_tab_to_new_window(
        &mut self,
        tab_id: Uuid,
        at: Option<Point>,
    ) -> Task<Message> {
        self.overlay = None;
        if !self.can_detach(tab_id) {
            return Task::none();
        }
        let Some((r, select)) = self.release_tab_ref(tab_id) else {
            return Task::none();
        };
        let view = match r {
            TabRef::Sftp(_) => View::Sftp,
            _ => View::Terminal,
        };
        let (_, open) = self.open_extra_window(vec![r], view, at);
        Task::batch([select, open])
    }

    /// Move a tab from the current window into `target` (`None` is the
    /// resident window), and bring that window forward on it. A window
    /// the move leaves with nothing in its strip closes.
    pub(super) fn move_tab_to_window(
        &mut self,
        tab_id: Uuid,
        target: Option<window::Id>,
    ) -> Task<Message> {
        self.overlay = None;
        let current = self.window_ctx.as_ref().map(|c| c.id);
        let exists = match target {
            Some(id) => self.extra_windows.contains_key(&id),
            None => current.is_some(),
        };
        if !exists || target == current {
            return Task::none();
        }
        let Some((r, select)) = self.release_tab_ref(tab_id) else {
            return Task::none();
        };
        self.give_ref_to_window(target, r, true);
        self.pending_focus = Some(target);
        if self.tab_order.is_empty() {
            return self.close_current_window(target);
        }
        select
    }

    /// "Duplicate in New Window": the ordinary duplicate, run as a new
    /// window, so the copy is born there.
    pub(super) fn duplicate_in_new_window(&mut self, idx: usize) -> Task<Message> {
        self.overlay = None;
        if self.tabs.get(idx).is_none() {
            return Task::none();
        }
        let (win, open) = self.open_extra_window(Vec::new(), View::Dashboard, None);
        let dial = self.run_in_window(Some(win), |s| s.handle_duplicate_tab(idx));
        Task::batch([open, dial])
    }

    /// "New Window": one more window, on the hosts screen.
    pub(super) fn spawn_new_window(&mut self) -> Task<Message> {
        let (_, open) = self.open_extra_window(Vec::new(), View::Dashboard, None);
        open
    }

    /// "Connect in New Window" from the card menu, and a host card
    /// dragged out of the window: one new window whose tabs are the
    /// given hosts, dialled one at a time through the batch queue (the
    /// prompts ride single slots).
    pub(super) fn connect_hosts_in_new_window(&mut self, ids: &[Uuid]) -> Task<Message> {
        self.card_context_menu = None;
        self.overlay = None;
        let ids: Vec<Uuid> = ids
            .iter()
            .copied()
            .filter(|id| self.connections.iter().any(|c| c.id == *id))
            .collect();
        if ids.is_empty() {
            return Task::none();
        }
        // The hosts left for another window, so the selection ends, the
        // way connecting them here ends it.
        self.nav.dash_selection.clear();
        let (win, open) = self.open_extra_window(Vec::new(), View::Dashboard, None);
        for id in ids {
            if !self.batch_dials.contains(&id) {
                self.batch_dials.push_back(id);
                self.batch_dial_windows.insert(id, win);
            }
        }
        // The queue starts in the funnel of this same update.
        open
    }

    /// The close verb on a window while others are open: its tabs close
    /// with it, so a live session or unsaved SFTP work asks first.
    /// Pinned tabs are not counted: they move to another window.
    pub(super) fn request_close_this_window(&mut self) -> Task<Message> {
        self.overlay = None;
        let doomed: Vec<usize> = (0..self.tabs.len())
            .filter(|&i| !self.tabs[i].pinned && self.shows_tab(i))
            .collect();
        let sftp: Vec<usize> = (0..self.sftp_tabs.len())
            .filter(|&i| !self.sftp_tabs[i].pinned && self.shows_sftp_tab(i))
            .collect();
        let live = self.live_session_count(&doomed)
            + sftp.iter().filter(|&&i| self.sftp_tab_is_live(i)).count();
        let unsaved = sftp.iter().any(|&i| self.sftp_tab_has_unsaved(i));
        if live == 0 && !unsaved {
            return self.close_this_window_now();
        }
        // What is at stake, in the dialog's own words: the sessions that
        // end, the SFTP work that is lost, or both.
        let mut body = String::new();
        if live > 0 {
            body = crate::i18n::t("close_one_window_body").replacen("{n}", &live.to_string(), 1);
        }
        if unsaved {
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str(crate::i18n::t("sftp_close_guard_detail"));
        }
        self.error_dialog = Some(crate::state::ErrorDialog {
            title: crate::i18n::t("close_one_window_title").to_string(),
            body,
            link: None,
            action: Some(crate::state::ErrorDialogAction {
                label: crate::i18n::t("close_one_window_confirm").to_string(),
                message: Box::new(Message::Tabs(TabsMessage::ConfirmCloseWindow)),
                danger: true,
            }),
        });
        Task::none()
    }

    /// Close the current window while others are open, with no prompt:
    /// every tab of its strip is closed through the user close (so the
    /// reopen stack can bring it back in another window), its panel
    /// chips go, and its pinned tabs move to another window.
    pub(super) fn close_this_window_now(&mut self) -> Task<Message> {
        self.overlay = None;
        let Some(heir) = self.other_windows().first().copied() else {
            return Task::none();
        };
        for r in self.tab_order.clone() {
            match r {
                TabRef::Terminal(id) => {
                    let Some(idx) = self.tabs.iter().position(|t| t._id == id) else {
                        continue;
                    };
                    if self.tabs[idx].pinned {
                        if let Some((r, _)) = self.release_tab_ref(id) {
                            self.give_ref_to_window(heir, r, false);
                        }
                    } else {
                        self.remember_closed_tab(idx);
                        self.teardown_tab_at(idx);
                    }
                }
                TabRef::Sftp(id) => {
                    let Some(idx) = self.sftp_tabs.iter().position(|t| t.id == id) else {
                        continue;
                    };
                    if self.sftp_tabs[idx].pinned {
                        if let Some((r, _)) = self.release_tab_ref(id) {
                            self.give_ref_to_window(heir, r, false);
                        }
                    } else {
                        self.remember_closed_sftp_tab(idx);
                        // Its own follow-up only navigates this window,
                        // which is on its way out.
                        let _ = self.close_sftp_tab(idx);
                    }
                }
                TabRef::Panel(kind) => {
                    let _ = self.close_panel_tab(kind);
                }
            }
        }
        self.tab_order.clear();
        self.active_tab = None;
        self.active_view = View::Dashboard;
        self.close_current_window(heir)
    }

    /// Take the current window off the screen, its strip already empty.
    /// An extra window is closed by `window_housekeeping`; the resident
    /// one hands its place to `prefer` (or any other window) first.
    fn close_current_window(&mut self, prefer: Option<window::Id>) -> Task<Message> {
        if let Some(id) = self.window_ctx.as_ref().map(|c| c.id) {
            self.windows_closing.push(id);
            return Task::none();
        }
        let next = prefer
            .filter(|id| self.extra_windows.contains_key(id))
            .or_else(|| self.extra_windows.keys().next().copied());
        let Some(next) = next else {
            return Task::none();
        };
        match self.promote_window(next) {
            Some(old) => window::close(old),
            None => Task::none(),
        }
    }

    /// A drag that crossed the window's edge: the dragged tab moves to
    /// a window of its own, the dragged host cards connect in one.
    /// `Some` when a drag was consumed.
    ///
    /// Read on every cursor move while a drag is up. The cursor keeps
    /// reporting past the edge for as long as the button is held, which
    /// is the only signal there is: the release lands outside and is
    /// never delivered.
    pub(super) fn tear_off_drag_at_edge(&mut self) -> Option<Task<Message>> {
        if !self.cursor_beyond_window(TEAR_OFF_MARGIN) {
            return None;
        }
        if self.card_drag.as_ref().is_some_and(|d| d.active) {
            let drag = self.card_drag.take()?;
            return Some(self.connect_hosts_in_new_window(&drag.ids));
        }
        let drag = self.tab_drag.filter(|d| d.active)?;
        // Terminal and SFTP tabs; a panel moves with whoever asks for it.
        self.strip_ref(drag.from_id)?;
        if !self.can_detach(drag.from_id) {
            return None;
        }
        self.tab_drag = None;
        let tab_id = drag.from_id;
        let cursor = self.mouse_position;
        // The window opens under the cursor, which is known in this
        // window's coordinates: ask where this window is. No answer
        // (Wayland has no positions) lets the compositor place it.
        Some(
            self.cur_window_task()
                .then(|id| match id {
                    Some(id) => window::position(id),
                    None => Task::done(None),
                })
                .map(move |origin| {
                    let at = origin.map(|o| {
                        Point::new(
                            o.x + cursor.x - DROP_OFFSET.x,
                            o.y + cursor.y - DROP_OFFSET.y,
                        )
                    });
                    Message::Tabs(TabsMessage::DetachTabAt(tab_id, at))
                }),
        )
    }

    /// The frame verbs of a window that is not the resident one. `Err`
    /// hands the message back for the resident window's handlers (and
    /// for everything that is not a frame verb): those carry the
    /// remembered geometry, the tray and the exit path.
    pub(super) fn handle_extra_window_chrome(
        &mut self,
        message: TabsMessage,
    ) -> Result<Task<Message>, TabsMessage> {
        let Some(id) = self.window_ctx.as_ref().map(|c| c.id) else {
            return Err(message);
        };
        let task = match message {
            TabsMessage::WindowResized(size) => {
                self.window_size = size;
                // The OS may have maximized or restored it (a snap, a
                // double click on the bar): ask, so the frame draws the
                // right glyph and resize edges.
                window::is_maximized(id).map(move |maximized| {
                    Message::Tabs(TabsMessage::WindowStateSynced {
                        maximized,
                        fullscreen: None,
                        size,
                    })
                })
            }
            TabsMessage::WindowStateSynced { maximized, .. } => {
                self.window_maximized = maximized;
                Task::none()
            }
            TabsMessage::WindowFocusChanged(focused) => {
                self.window_focused = focused;
                if focused {
                    self.input_window = Some(id);
                }
                if !focused {
                    // Same reason as the resident window: a release outside
                    // never arrives, so losing focus ends the drag.
                    self.tab_drag = None;
                }
                Task::none()
            }
            TabsMessage::WindowDrag => {
                if self.consume_window_press() {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            TabsMessage::WindowResizeDrag(direction) => {
                if !self.window_maximized && self.consume_window_press() {
                    window::drag_resize(id, direction)
                } else {
                    Task::none()
                }
            }
            TabsMessage::WindowMinimize => window::minimize(id, true),
            TabsMessage::WindowMaximizeToggle => {
                self.window_maximized = !self.window_maximized;
                window::toggle_maximize(id)
            }
            TabsMessage::WindowFullscreenToggle => {
                self.window_fullscreen = !self.window_fullscreen;
                self.fullscreen_immersive = self.window_fullscreen;
                window::set_mode(
                    id,
                    if self.window_fullscreen {
                        window::Mode::Fullscreen
                    } else {
                        window::Mode::Windowed
                    },
                )
            }
            TabsMessage::WindowClose => self.request_close_this_window(),
            TabsMessage::ConfirmCloseWindow => self.close_this_window_now(),
            // The remembered geometry, the on-screen rescue and the
            // macOS fullscreen reconciliation: none of it is kept for
            // an extra window.
            TabsMessage::WindowMoved(_)
            | TabsMessage::WindowEnsureOnScreen
            | TabsMessage::WindowExpandVertical
            | TabsMessage::WindowFullscreenSettled(_)
            | TabsMessage::FullscreenHintHide => Task::none(),
            other => return Err(other),
        };
        Ok(task)
    }
}
