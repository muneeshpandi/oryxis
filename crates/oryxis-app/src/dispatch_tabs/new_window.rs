//! Extra windows: opening one, moving tabs in and out, and its frame.
//!
//! The model is in `window_ctx`: every tab lives in `Oryxis::tabs`, a
//! window only lists which ones it shows. So nothing here touches a
//! session. Moving a tab (terminal or SFTP) is moving its strip ref from
//! one list to another, and a window that closes hands its refs back to
//! the main strip.
//!
//! Each function runs with the SOURCE window's values in the per-window
//! fields (`tab_order`, `active_tab`): that is the window the gesture
//! came from, and it is how the same code serves a tab leaving the main
//! window and a tab leaving an extra one.

use iced::{window, Point, Task};
use uuid::Uuid;

use crate::app::{Message, Oryxis, SettingsMessage, SftpMessage, TabsMessage};
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
    /// Register a new extra window showing `order` and return the task
    /// that opens it. The id is known at once; nothing is on screen
    /// until the task runs, so a caller that ends up with nothing to
    /// show can drop both.
    fn open_extra_window(
        &mut self,
        order: Vec<TabRef>,
        at: Option<Point>,
        awaiting_first_tab: bool,
    ) -> (window::Id, Task<Message>) {
        let mut settings = crate::app::MAIN_WINDOW_SETTINGS
            .get()
            .cloned()
            .unwrap_or_default();
        // The size of the window the tab is leaving, so the terminal
        // keeps its grid; never its maximized or fullscreen state.
        settings.size = self.window_size;
        settings.maximized = false;
        settings.fullscreen = false;
        settings.position = match at {
            Some(p) => window::Position::Specific(p),
            None => window::Position::Default,
        };
        let (id, open) = window::open(settings);
        let mut extra = ExtraWindow::new(order, self.window_size);
        extra.awaiting_first_tab = awaiting_first_tab;
        self.extra_windows.insert(id, extra);
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
    /// neighbour, through the ordinary select of its kind. With no tab
    /// next to it the main window goes home; an extra window is about
    /// to close.
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

    /// Move a tab (terminal or SFTP, by strip id) out of the current
    /// window into a new extra window, opened at `at` (screen
    /// coordinates) when given.
    pub(super) fn detach_tab_to_new_window(
        &mut self,
        tab_id: Uuid,
        at: Option<Point>,
    ) -> Task<Message> {
        self.overlay = None;
        // The only tab of an extra window is already in a window of its
        // own.
        if self.window_ctx.is_some() && self.tab_order.len() <= 1 {
            return Task::none();
        }
        let Some((r, select)) = self.release_tab_ref(tab_id) else {
            return Task::none();
        };
        let (_, open) = self.open_extra_window(vec![r], at, false);
        Task::batch([select, open])
    }

    /// Move a tab from the current (extra) window back into the main
    /// one, and bring the main window forward on it.
    pub(super) fn move_tab_to_main_window(&mut self, tab_id: Uuid) -> Task<Message> {
        self.overlay = None;
        if self.window_ctx.is_none() {
            return Task::none();
        }
        let Some((r, select)) = self.release_tab_ref(tab_id) else {
            return Task::none();
        };
        self.park_ref_in_main(r, true);
        self.pending_focus_main = true;
        select
    }

    /// "Duplicate in New Window": the ordinary duplicate, run as a new
    /// extra window, so the copy is born there.
    pub(super) fn duplicate_in_new_window(&mut self, idx: usize) -> Task<Message> {
        self.overlay = None;
        if self.tabs.get(idx).is_none() {
            return Task::none();
        }
        let (win, open) = self.open_extra_window(Vec::new(), None, true);
        let dial = self.run_in_window(Some(win), |s| s.handle_duplicate_tab(idx));
        Task::batch([open, dial])
    }

    /// "New Window": an extra window with a fresh local shell.
    pub(super) fn spawn_new_window(&mut self) -> Task<Message> {
        let (win, open) = self.open_extra_window(Vec::new(), None, true);
        let shell = self.run_in_window(Some(win), |s| {
            s.update(Message::Settings(SettingsMessage::OpenLocalShell))
        });
        Task::batch([open, shell])
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
        self.dash_selection.clear();
        let (win, open) = self.open_extra_window(Vec::new(), None, true);
        for id in ids {
            if !self.batch_dials.contains(&id) {
                self.batch_dials.push_back(id);
                self.batch_dial_windows.insert(id, win);
            }
        }
        // The queue starts in the funnel of this same update.
        open
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
        // Terminal and SFTP tabs; a panel belongs to the main window.
        self.strip_ref(drag.from_id)?;
        if self.window_ctx.is_some() && self.tab_order.len() <= 1 {
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

    /// The frame verbs of an EXTRA window. `Err` hands the message back
    /// for the main window's handlers (and for everything that is not a
    /// frame verb).
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
                    // Same reason as the main window: a release outside
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
            // Closing an extra window ends no session: its tabs go back
            // to the main strip, and the emptied window is closed by
            // `window_housekeeping`.
            TabsMessage::WindowClose | TabsMessage::ConfirmCloseWindow => {
                self.overlay = None;
                // The SFTP surface this window had hoisted goes home to
                // its slot before its ref changes strip.
                if self.hybrid_sftp_owner.is_some() {
                    self.park_hybrid_sftp();
                }
                if let Some(idx) = self.active_sftp.take()
                    && let Some(tab) = self.sftp_tabs.get_mut(idx)
                {
                    tab.state = std::mem::take(&mut self.sftp);
                }
                for r in std::mem::take(&mut self.tab_order) {
                    self.park_ref_in_main(r, false);
                }
                self.active_tab = None;
                self.active_view = View::Terminal;
                Task::none()
            }
            // The main window's geometry memory, on-screen rescue and
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
