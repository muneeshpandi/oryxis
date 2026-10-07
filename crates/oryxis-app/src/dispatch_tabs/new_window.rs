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

/// Where a carried window's corner sits relative to the cursor when the
/// tab's chip was never drawn (`torn_off_grab` has nothing to measure),
/// and the offset handed to a compositor's carry: the chip of the tab
/// ends up roughly under the pointer.
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
        // keeps its grid; never its maximized or fullscreen state, and
        // from a maximized or fullscreen source its WINDOWED size (the
        // rectangle on screen is the monitor's).
        settings.size = if self.cur_maximized() || self.cur_fullscreen() {
            self.window_windowed_size
        } else {
            self.window_size
        };
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
        self.move_tab_to_window_at(tab_id, target, None)
    }

    /// [`Self::move_tab_to_window`], landing at a slot of the target's
    /// strip as drawn instead of at its end.
    pub(super) fn move_tab_to_window_at(
        &mut self,
        tab_id: Uuid,
        target: Option<window::Id>,
        slot: Option<usize>,
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
        self.give_ref_to_window_at(target, r, true, slot);
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
                message: Box::new(Message::Tabs(TabsMessage::ConfirmCloseWindow {
                    whole_app: false,
                })),
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
            // The promoted window is now the one whose geometry is
            // remembered, and nothing has told us where it sits (an
            // extra window's moves are not tracked): ask, and let the
            // answer land as a Moved event through the resident's own
            // guards, or the next launch would open this window's size
            // at the closed window's place.
            Some(old) => Task::batch([
                window::close(old),
                window::position(next).map(|pos| match pos {
                    Some(pos) => Message::Tabs(TabsMessage::WindowMoved(pos)),
                    None => Message::NoOp,
                }),
            ]),
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
    ///
    /// A tab that is the only one in a window that is one of several
    /// does not wait for the edge: the window IS that tab, so dragging
    /// the tab drags the window, the moment the drag starts.
    pub(super) fn tear_off_drag_at_edge(&mut self) -> Option<Task<Message>> {
        let beyond = self.cursor_beyond_window(TEAR_OFF_MARGIN);
        if beyond && self.card_drag.as_ref().is_some_and(|d| d.active) {
            let drag = self.card_drag.take()?;
            return Some(self.connect_hosts_in_new_window(&drag.ids));
        }
        let drag = self.tab_drag.filter(|d| d.active)?;
        // Terminal and SFTP tabs; a panel moves with whoever asks for it.
        self.strip_ref(drag.from_id)?;
        let whole = self.is_whole_window(drag.from_id);
        if !beyond && !whole {
            return None;
        }
        if !whole && !self.can_detach(drag.from_id) {
            return None;
        }
        self.tab_drag = None;
        let tab_id = drag.from_id;
        // The press is still down: the answer below starts a carry only
        // if no release came in between.
        self.tear_off_pending = Some((tab_id, drag.start));
        // Where every window is on the screen: the carried one is put
        // under the cursor (known in THIS window's coordinates) and can
        // dock on another's strip. No answer (Wayland has no positions)
        // leaves the carrying to the compositor.
        let queries = self
            .all_windows()
            .into_iter()
            .map(|(_, id)| window::position(id).map(move |position| (id, position)));
        Some(
            Task::batch(queries)
                .collect()
                .map(move |positions| Message::Tabs(TabsMessage::DetachTabAt(tab_id, positions))),
        )
    }

    /// Whether the tab `id` is all the current window holds while other
    /// windows are open: dragging it out carries the window itself.
    fn is_whole_window(&self, id: Uuid) -> bool {
        self.window_count() > 1 && self.tab_order.iter().all(|r| r.strip_id() == id)
    }

    /// The id of the current window, the resident one included.
    fn current_window_id(&self) -> Option<window::Id> {
        self.window_ctx
            .as_ref()
            .map(|c| c.id)
            .or_else(crate::app::resident_window_id)
    }

    /// The per-window target naming the window `id` (`None` is the
    /// resident one).
    fn target_of(id: window::Id) -> Option<window::Id> {
        (crate::app::resident_window_id() != Some(id)).then_some(id)
    }

    /// Whether the window `id` is still open.
    fn window_alive(&self, id: window::Id) -> bool {
        self.all_windows().iter().any(|(_, w)| *w == id)
    }

    /// The tab `tab_id` was dragged out of the current window: carry it,
    /// in a new window, or in its own when it was the only tab there.
    pub(super) fn tear_off_tab(
        &mut self,
        tab_id: Uuid,
        positions: Vec<(window::Id, Option<Point>)>,
    ) -> Task<Message> {
        // The release came while the positions were asked for: no carry.
        let pending = self.tear_off_pending.take();
        let press = pending.filter(|(id, _)| *id == tab_id).map(|(_, press)| press);
        let holder = self.window_ctx.as_ref().map(|c| c.id);
        let Some(holder_id) = self.current_window_id() else {
            return Task::none();
        };
        let whole = self.is_whole_window(tab_id);
        if whole && press.is_none() {
            return Task::none();
        }
        let position_of = |id: window::Id| {
            positions
                .iter()
                .find(|(w, _)| *w == id)
                .and_then(|(_, position)| *position)
        };
        let origin = position_of(holder_id);
        let docks_without = |s: &Self, carried: window::Id| -> Vec<(Option<window::Id>, iced::Rectangle)> {
            s.all_windows()
                .into_iter()
                .filter(|(_, id)| *id != carried)
                .filter_map(|(target, id)| {
                    let position = position_of(id)?;
                    Some((target, iced::Rectangle::new(position, s.size_of_window(target))))
                })
                .collect()
        };
        if whole {
            let press = press.unwrap_or(DROP_OFFSET);
            let Some(origin) = origin else {
                // No positions (Wayland): the compositor carries the
                // window if it has the protocol for that.
                return window::drag_toplevel(holder_id, holder_id, press).map(move |started| {
                    Message::Tabs(TabsMessage::ToplevelDragStarted {
                        started,
                        window: holder_id,
                        tab: tab_id,
                    })
                });
            };
            self.window_carry = Some(crate::window_ctx::WindowCarry {
                window: holder_id,
                tab: tab_id,
                holder,
                origin,
                grab: press,
                hidden: false,
                docks: docks_without(self, holder_id),
                native: false,
                hover: None,
            });
            return Task::none();
        }
        let Some(origin) = origin else {
            // No positions (Wayland): the app cannot put a window under
            // the cursor, but the compositor can carry it if it has the
            // protocol for that. Asked once the window is open; a "no"
            // leaves the window where the compositor placed it.
            let before: Vec<window::Id> = self.extra_windows.keys().copied().collect();
            let open = self.detach_tab_to_new_window(tab_id, None);
            let carried = self
                .extra_windows
                .keys()
                .copied()
                .find(|id| !before.contains(id));
            let Some(carried) = carried.filter(|_| press.is_some()) else {
                return open;
            };
            return open.chain(
                window::drag_toplevel(holder_id, carried, DROP_OFFSET).map(move |started| {
                    Message::Tabs(TabsMessage::ToplevelDragStarted {
                        started,
                        window: carried,
                        tab: tab_id,
                    })
                }),
            );
        };
        let cursor = self.mouse_position;
        let grab = self.torn_off_grab(tab_id);
        let at = Point::new(origin.x + cursor.x - grab.x, origin.y + cursor.y - grab.y);
        let before: Vec<window::Id> = self.extra_windows.keys().copied().collect();
        let open = self.detach_tab_to_new_window(tab_id, Some(at));
        let carried = self
            .extra_windows
            .keys()
            .copied()
            .find(|id| !before.contains(id));
        // Released already: the window opens where the tab was let go.
        let Some(carried) = carried.filter(|_| press.is_some()) else {
            return open;
        };
        self.window_carry = Some(crate::window_ctx::WindowCarry {
            window: carried,
            tab: tab_id,
            holder,
            origin,
            grab,
            hidden: false,
            docks: docks_without(self, carried),
            native: false,
            hover: None,
        });
        open
    }

    /// Where, in the window a tab is torn off into, the cursor has to be
    /// for the tab's chip to sit centred under it, before that window
    /// has drawn its strip (from then on the carry reads the chip
    /// itself, see `carry_window_to_cursor`). The new window draws the
    /// same strip with that tab alone: the chip takes the place of this
    /// strip's first one, at the width a lone tab gets. Read before the
    /// tab leaves this strip.
    fn torn_off_grab(&self, tab_id: Uuid) -> Point {
        let here = self.cur_window();
        let Some(chip) = self.strip_chip_rect(here, tab_id) else {
            return DROP_OFFSET;
        };
        let first = self
            .tab_order
            .iter()
            .filter_map(|r| self.strip_chip_rect(here, r.strip_id()))
            .fold(chip, |first, r| iced::Rectangle {
                x: first.x.min(r.x),
                y: first.y.min(r.y),
                ..first
            });
        let pinned = self.ref_of_tab(tab_id).is_some_and(|r| self.ref_pinned(&r));
        let across = !crate::views::tab_bar::tab_bar_pos().is_side();
        // A lone tab on a horizontal strip is drawn at its natural width
        // by the adaptive sizing; a compact pin and the uniform sizing
        // keep the width it has here.
        let width = if across
            && !(pinned && self.prefs.pinned_tab_style == "compact")
            && self.prefs.tab_width_mode != "uniform"
        {
            crate::views::tab_bar::TAB_NATURAL_WIDTH
        } else {
            chip.width
        };
        Point::new(first.x + width / 2.0, first.y + chip.height / 2.0)
    }

    /// The compositor answered whether it carries the torn-off window.
    pub(super) fn native_carry_started(&mut self, started: bool, window: window::Id, tab: Uuid) {
        if !started || !self.window_alive(window) {
            return;
        }
        self.window_carry = Some(crate::window_ctx::WindowCarry {
            window,
            tab,
            holder: self.window_ctx.as_ref().map(|c| c.id),
            origin: Point::ORIGIN,
            grab: DROP_OFFSET,
            hidden: false,
            docks: Vec::new(),
            native: true,
            hover: None,
        });
    }

    /// A window the compositor carries is over the current window at
    /// `position`, or (`None`) just left it. Over the strip, the strip
    /// shows the tab at the slot it would take.
    pub(super) fn native_carry_hovered(&mut self, position: Option<Point>) {
        let here = self.window_ctx.as_ref().map(|c| c.id);
        let Some(carry) = self.window_carry.as_ref().filter(|c| c.native) else {
            return;
        };
        let (carried, tab) = (carry.window, carry.tab);
        let over_strip = position.filter(|p| {
            // A tab does not dock into its own window.
            self.current_window_id() != Some(carried)
                && crate::views::tab_bar::cursor_in_tab_strip_band(
                    crate::views::tab_bar::tab_bar_pos(),
                    *p,
                    self.window_size,
                    self.prefs.pinned_tabs_top_bar && !self.top_bar_hidden(),
                )
        });
        let hover = over_strip.map(|cursor| crate::window_ctx::CarryHover {
            window: here,
            cursor,
            slot: self.carry_slot(here, tab, cursor),
        });
        let Some(carry) = self.window_carry.as_mut() else {
            return;
        };
        match hover {
            Some(hover) => carry.hover = Some(hover),
            None => {
                if carry.hover.is_some_and(|h| h.window == here) {
                    carry.hover = None;
                }
            }
        }
    }

    /// The compositor's carry ended. Over another window's strip the
    /// tab docks there; anywhere else its window stays where it was let
    /// go. Reported twice for a drop on a window (by that window and by
    /// the one the drag started in): the second finds nothing to do.
    pub(super) fn native_carry_released(&mut self) -> Task<Message> {
        if !self.window_carry.as_ref().is_some_and(|c| c.native) {
            return Task::none();
        }
        let Some(carry) = self.window_carry.take() else {
            return Task::none();
        };
        let Some(hover) = carry.hover else {
            return Task::none();
        };
        if !self.window_alive(carry.window) {
            return Task::none();
        }
        let tab = carry.tab;
        self.run_in_window(Self::target_of(carry.window), |s| {
            s.move_tab_to_window_at(tab, hover.window, Some(hover.slot))
        })
    }

    /// While a window is carried: over another window's strip, hide it
    /// and let that strip show the tab; anywhere else, show it under
    /// the cursor. `Some` when the move belongs to the carry (it came
    /// from the window the button is held in).
    pub(super) fn carry_window_to_cursor(&mut self) -> Option<Task<Message>> {
        let carry = self.window_carry.as_ref()?;
        // A window the compositor carries needs no help, and the holder
        // gets no pointer for it anyway.
        if carry.native || carry.holder != self.window_ctx.as_ref().map(|c| c.id) {
            return None;
        }
        if !self.window_alive(carry.window) {
            self.window_carry = None;
            return None;
        }
        let screen = Point::new(
            carry.origin.x + self.mouse_position.x,
            carry.origin.y + self.mouse_position.y,
        );
        // A geometry test, like the file drag-out's: a window lying over
        // another one is not told apart from it.
        let pins_top = self.prefs.pinned_tabs_top_bar && !self.top_bar_hidden();
        let over = carry.docks.iter().find_map(|(target, rect)| {
            let local = Point::new(screen.x - rect.x, screen.y - rect.y);
            (rect.contains(screen)
                && crate::views::tab_bar::cursor_in_tab_strip_band(
                    crate::views::tab_bar::tab_bar_pos(),
                    local,
                    rect.size(),
                    pins_top,
                ))
            .then_some((*target, local))
        });
        let hover = over.map(|(window, cursor)| crate::window_ctx::CarryHover {
            window,
            cursor,
            slot: self.carry_slot(window, carry.tab, cursor),
        });
        let drawn_chip = self.strip_chip_rect(Some(carry.window), carry.tab);
        let carry = self.window_carry.as_mut()?;
        carry.hover = hover;
        if hover.is_some() {
            if carry.hidden {
                return Some(Task::none());
            }
            carry.hidden = true;
            return Some(window::set_mode(carry.window, window::Mode::Hidden));
        }
        // A torn-off window holds its tab's chip under the cursor: once
        // it has drawn its strip, by the chip itself. (A window carried
        // by its only tab keeps the point it was pressed at.)
        let whole = Some(carry.window) == carry.holder.or_else(crate::app::resident_window_id);
        if !whole
            && !carry.hidden
            && let Some(chip) = drawn_chip
        {
            carry.grab = Point::new(chip.center_x(), chip.center_y());
        }
        let at = Point::new(screen.x - carry.grab.x, screen.y - carry.grab.y);
        // The holder's coordinates move with it when it is the window
        // being carried.
        if whole {
            carry.origin = at;
        }
        let mut task = window::move_to(carry.window, at);
        if carry.hidden {
            carry.hidden = false;
            task = task.chain(window::set_mode(carry.window, window::Mode::Windowed));
        }
        Some(task)
    }

    /// The release that ends a window carry. Over another window's tab
    /// strip the tab docks there, at the slot the strip showed (and its
    /// emptied window closes); anywhere else the carried window simply
    /// stays. `None` when no carry was in flight for this window.
    pub(crate) fn finish_window_carry(&mut self) -> Option<Task<Message>> {
        // A release before the positions answered: the tear-off opens
        // no carry (see `tear_off_tab`).
        self.tear_off_pending = None;
        let holder = self.window_ctx.as_ref().map(|c| c.id);
        let carry = self.window_carry.as_ref()?;
        // A compositor's carry ends with its own events
        // (`native_carry_released`), not with a button release.
        if carry.native || carry.holder != holder {
            return None;
        }
        let carry = self.window_carry.take()?;
        if !self.window_alive(carry.window) {
            return Some(Task::none());
        }
        let Some(hover) = carry.hover else {
            return Some(Task::none());
        };
        let tab = carry.tab;
        Some(self.run_in_window(Self::target_of(carry.window), |s| {
            s.move_tab_to_window_at(tab, hover.window, Some(hover.slot))
        }))
    }

    /// A carry whose release never arrived (the platform did not keep
    /// the pointer with the window the press began in) ends at the next
    /// press, and a window it hid comes back.
    pub(crate) fn abandon_window_carry(&mut self) {
        self.tear_off_pending = None;
        if let Some(carry) = self.window_carry.take()
            && carry.hidden
        {
            self.windows_revealing.push(carry.window);
        }
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
                // Same 8 px grid as the resident window, for the same
                // reason: a drag-resize must not reflow every pixel.
                let snapped = Self::snapped_window_size(size);
                if (snapped.width - self.window_size.width).abs() <= 0.5
                    && (snapped.height - self.window_size.height).abs() <= 0.5
                {
                    return Ok(Task::none());
                }
                self.window_size = snapped;
                self.dismiss_width_anchored_popovers();
                // The OS may have maximized or restored it (a snap, a
                // double click on the bar): ask, so the frame draws the
                // right glyph and resize edges. Under the native macOS
                // frame the green button is a door into fullscreen that
                // sends no message of ours, and this window's gutter and
                // chrome read `cur_fullscreen()`, so the mode is asked
                // for too. The settled re-read 900 ms later is the
                // resident's; an extra window takes the next resize.
                window::is_maximized(id).then(move |maximized| {
                    let synced = move |fullscreen| {
                        Message::Tabs(TabsMessage::WindowStateSynced {
                            maximized,
                            fullscreen,
                            size: snapped,
                        })
                    };
                    if crate::views::chrome::NATIVE_FRAME {
                        window::mode(id)
                            .map(move |mode| synced(Some(mode == window::Mode::Fullscreen)))
                    } else {
                        Task::done(synced(None))
                    }
                })
            }
            TabsMessage::WindowStateSynced { maximized, fullscreen, .. } => {
                if let Some(fullscreen) = fullscreen {
                    self.reconcile_window_fullscreen(fullscreen);
                }
                self.window_maximized = maximized;
                Task::none()
            }
            TabsMessage::WindowFocusChanged(focused) => {
                self.window_focused = focused;
                if focused {
                    self.input_window = Some(id);
                }
                if !focused {
                    self.on_window_blur();
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
                let mode_task = window::set_mode(
                    id,
                    if self.window_fullscreen {
                        window::Mode::Fullscreen
                    } else {
                        window::Mode::Windowed
                    },
                );
                // Same "press F11 to exit" hint as the resident window,
                // drawn only by the window that is immersive.
                if self.window_fullscreen {
                    self.fullscreen_hint_visible = true;
                    let hide_task = Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        },
                        |_| Message::Tabs(TabsMessage::FullscreenHintHide),
                    );
                    Task::batch([mode_task, hide_task])
                } else {
                    self.fullscreen_hint_visible = false;
                    mode_task
                }
            }
            TabsMessage::WindowClose => self.request_close_this_window(),
            // An extra window is never the app's close; a confirmation
            // that asked for the app's close was raised by a window that
            // was the last one at the time and is not any more.
            TabsMessage::ConfirmCloseWindow { whole_app } => {
                if whole_app {
                    self.handle_window_close()
                } else {
                    self.close_this_window_now()
                }
            }
            // The remembered geometry, the on-screen rescue and the
            // macOS fullscreen reconciliation: none of it is kept for
            // an extra window.
            TabsMessage::FullscreenHintHide => {
                self.fullscreen_hint_visible = false;
                Task::none()
            }
            TabsMessage::WindowMoved(_)
            | TabsMessage::WindowEnsureOnScreen
            | TabsMessage::WindowExpandVertical
            | TabsMessage::WindowFullscreenSettled(_) => Task::none(),
            other => return Err(other),
        };
        Ok(task)
    }
}
