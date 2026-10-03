//! More than one window, one application.
//!
//! The app is an iced daemon: one process, one `Oryxis`, any number of
//! windows. The MAIN window is the whole app (vault views, settings,
//! every tab). An EXTRA window holds terminal tabs and nothing else: a
//! tab dragged out of a strip, or opened there on purpose. Nothing about
//! a tab lives in the window: every `TerminalTab` stays in
//! `Oryxis::tabs`, with its session, scrollback and panes, and a window
//! only says which of them it shows and in what order. Moving a tab
//! between windows therefore moves an id between two lists, and the
//! session never notices.
//!
//! What IS per window is small: the strip order, the active tab, the
//! size, the cursor, the maximized / fullscreen flags. The main window
//! keeps those in the `Oryxis` fields they always lived in. An extra
//! window keeps its own copy in [`ExtraWindow`], and reaches the code
//! that reads those fields in two ways:
//!
//! - `update`: a message that comes from an extra window arrives as
//!   `Message::InWindow(id, ..)`, and [`Oryxis::update_in_window`] SWAPS
//!   that window's values into the fields for the duration, so every
//!   handler acts on "the active tab" and "the strip" of the window the
//!   user is in without knowing there is more than one.
//! - `view`: built from `&self`, where nothing can be swapped, so the
//!   window being drawn is ambient ([`viewing`]) and the views read the
//!   per-window values through the `cur_*` accessors below. During
//!   `update` nothing is being drawn and the accessors return the
//!   fields, which is what makes them safe to use anywhere.

use std::cell::Cell;

use iced::window;
use iced::{Point, Size, Task};
use uuid::Uuid;

use crate::app::{Message, Oryxis};
use crate::state::{TabRef, View};

/// Whose view is under construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Viewing {
    /// None: this is `update`, or a test calling a view helper.
    Nothing,
    Main,
    Extra(window::Id),
}

thread_local! {
    static VIEWING: Cell<Viewing> = const { Cell::new(Viewing::Nothing) };
}

/// The extra window whose view is being built right now.
fn viewing() -> Option<window::Id> {
    match VIEWING.with(Cell::get) {
        Viewing::Extra(id) => Some(id),
        _ => None,
    }
}

/// Marks a view pass for its duration.
struct ViewPass;

impl ViewPass {
    fn begin(target: Viewing) -> Self {
        VIEWING.with(|v| v.set(target));
        Self
    }
}

impl Drop for ViewPass {
    fn drop(&mut self) {
        VIEWING.with(|v| v.set(Viewing::Nothing));
    }
}

/// What an extra window keeps for itself.
#[derive(Debug, Clone)]
pub(crate) struct ExtraWindow {
    /// Its strip, in order. Terminal refs only.
    pub(crate) order: Vec<TabRef>,
    /// The tab it shows, by `TerminalTab::_id`: an index into
    /// `Oryxis::tabs` would go stale the moment another window closed
    /// a tab.
    pub(crate) active: Option<Uuid>,
    pub(crate) size: Size,
    pub(crate) cursor: Point,
    pub(crate) maximized: bool,
    pub(crate) fullscreen: bool,
    pub(crate) immersive: bool,
    pub(crate) focused: bool,
    /// Opened for a tab that a dial is still about to create (hosts
    /// connected in a new window): the one moment an empty strip is not
    /// a reason to close. Cleared the first time a tab lands.
    pub(crate) awaiting_first_tab: bool,
    pub(crate) born: std::time::Instant,
    /// Its own most-recently-used order, so Ctrl+Tab cycles the tabs
    /// of the window it is pressed in.
    pub(crate) mru: Vec<TabRef>,
}

impl ExtraWindow {
    pub(crate) fn new(order: Vec<TabRef>, size: Size) -> Self {
        let active = order.iter().find_map(|r| match r {
            TabRef::Terminal(id) => Some(*id),
            _ => None,
        });
        Self {
            order,
            active,
            size,
            cursor: Point::ORIGIN,
            maximized: false,
            fullscreen: false,
            immersive: false,
            focused: true,
            awaiting_first_tab: false,
            born: std::time::Instant::now(),
            mru: Vec::new(),
        }
    }

    /// Still empty long after it was opened for a tab that never came.
    fn overdue(&self) -> bool {
        self.born.elapsed() > std::time::Duration::from_secs(30)
    }

    pub(crate) fn tab_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.order.iter().filter_map(|r| match r {
            TabRef::Terminal(id) => Some(*id),
            _ => None,
        })
    }
}

/// The main window's own values, set aside while an extra window's are
/// in the fields.
#[derive(Debug)]
pub(crate) struct WindowCtx {
    pub(crate) id: window::Id,
    order: Vec<TabRef>,
    active: Option<Uuid>,
    view: View,
    size: Size,
    cursor: Point,
    maximized: bool,
    fullscreen: bool,
    immersive: bool,
    focused: bool,
    /// The window's own `awaiting_first_tab` / `born`, carried across
    /// the swap.
    awaiting: (bool, std::time::Instant),
    mru: Vec<TabRef>,
}

impl Oryxis {
    /// The extra window being drawn, if the view under construction is
    /// one. Always `None` during `update`.
    fn viewed_extra(&self) -> Option<&ExtraWindow> {
        viewing().and_then(|id| self.extra_windows.get(&id))
    }

    /// The window a view or a handler is working for: the extra window
    /// being drawn, else the one whose values are swapped in, else
    /// `None` for the main window.
    pub(crate) fn cur_window(&self) -> Option<window::Id> {
        viewing()
            .filter(|id| self.extra_windows.contains_key(id))
            .or(self.window_ctx.as_ref().map(|c| c.id))
    }

    /// Whether the current window is an extra one.
    pub(crate) fn in_extra_window(&self) -> bool {
        self.cur_window().is_some()
    }

    pub(crate) fn cur_active_tab(&self) -> Option<usize> {
        match self.viewed_extra() {
            Some(w) => w
                .active
                .and_then(|id| self.tabs.iter().position(|t| t._id == id)),
            None => self.active_tab,
        }
    }

    pub(crate) fn cur_view(&self) -> View {
        match self.viewed_extra() {
            Some(_) => View::Terminal,
            None => self.active_view,
        }
    }

    pub(crate) fn cur_tab_order(&self) -> &[TabRef] {
        match self.viewed_extra() {
            Some(w) => &w.order,
            None => &self.tab_order,
        }
    }

    pub(crate) fn cur_window_size(&self) -> Size {
        self.viewed_extra().map_or(self.window_size, |w| w.size)
    }

    pub(crate) fn cur_mouse(&self) -> Point {
        self.viewed_extra().map_or(self.mouse_position, |w| w.cursor)
    }

    pub(crate) fn cur_maximized(&self) -> bool {
        self.viewed_extra().map_or(self.window_maximized, |w| w.maximized)
    }

    pub(crate) fn cur_fullscreen(&self) -> bool {
        self.viewed_extra().map_or(self.window_fullscreen, |w| w.fullscreen)
    }

    pub(crate) fn cur_immersive(&self) -> bool {
        self.viewed_extra().map_or(self.fullscreen_immersive, |w| w.immersive)
    }

    pub(crate) fn cur_focused(&self) -> bool {
        self.viewed_extra().map_or(self.window_focused, |w| w.focused)
    }

    /// The current window as a task, for "do this to the window the
    /// gesture came from". Resolved NOW: the task may run after the
    /// swap is undone.
    pub(crate) fn cur_window_task(&self) -> Task<Option<window::Id>> {
        match self.cur_window() {
            Some(id) => Task::done(Some(id)),
            None => crate::app::main_window(),
        }
    }

    /// Whether the terminal tab at `idx` is in the strip of the window
    /// the handler is running for. Always true with a single window.
    pub(crate) fn shows_tab(&self, idx: usize) -> bool {
        if self.extra_windows.is_empty() && self.window_ctx.is_none() {
            return true;
        }
        self.tabs
            .get(idx)
            .is_some_and(|t| self.tab_order.contains(&TabRef::Terminal(t._id)))
    }

    /// The window that shows the terminal tab `id`: `None` is the main
    /// one.
    pub(crate) fn window_of_tab(&self, id: Uuid) -> Option<window::Id> {
        if let Some(ctx) = &self.window_ctx
            && self.tab_order.contains(&TabRef::Terminal(id))
        {
            return Some(ctx.id);
        }
        self.extra_windows
            .iter()
            .find(|(_, w)| w.tab_ids().any(|t| t == id))
            .map(|(win, _)| *win)
    }

    /// Build a window's view. An extra window gets the ordinary root
    /// view with itself made ambient for the `cur_*` accessors, and its
    /// messages tagged with its id.
    pub(crate) fn view_for_window(&self, id: window::Id) -> iced::Element<'_, Message> {
        if self.extra_windows.contains_key(&id) {
            let _pass = ViewPass::begin(Viewing::Extra(id));
            self.view()
                .map(move |m| Message::InWindow(id, Box::new(m)))
        } else {
            // Any other id is the main window, which keeps the harness
            // (its emulator owns a single window) and a window on its
            // way out on the ordinary path.
            let _pass = ViewPass::begin(Viewing::Main);
            self.view()
        }
    }

    /// The window the user is working in (the focused one), when it is
    /// an extra window that still exists.
    fn input_window(&self) -> Option<window::Id> {
        self.input_window.filter(|w| {
            self.extra_windows.contains_key(w)
                || self.window_ctx.as_ref().is_some_and(|c| c.id == *w)
        })
    }

    /// Whether the current window is the one the user is working in.
    /// Toasts show there, and so does the keyboard-navigation ring.
    pub(crate) fn input_here(&self) -> bool {
        self.input_window() == self.cur_window()
    }

    /// Whether the floating layer (a context menu, a modal, a drag
    /// ghost) belongs to the current window: the one it was raised
    /// from. There is one of each in the app, so it is drawn once.
    pub(crate) fn floats_here(&self) -> bool {
        let owner = self.float_window.filter(|w| {
            self.extra_windows.contains_key(w)
                || self.window_ctx.as_ref().is_some_and(|c| c.id == *w)
        });
        owner == self.cur_window()
    }

    /// The keyboard-navigation state a view records into and draws its
    /// ring from. The views of every window are rebuilt after each
    /// update and they all record their rows; only the window the user
    /// is working in may write the real lists, or the ring would act on
    /// rows of a window nobody is looking at. The others get a scratch
    /// copy, which also reads as "nothing selected", so they draw no
    /// ring. Outside a view pass this is always the real state.
    pub(crate) fn kn(&self) -> &crate::keynav::KeyNavState {
        let real = match VIEWING.with(Cell::get) {
            Viewing::Nothing => true,
            Viewing::Main => self.input_window().is_none(),
            Viewing::Extra(id) => self.input_window() == Some(id),
        };
        if real { &self.keynav } else { &self.keynav_scratch }
    }

    /// What can float over a window right now, as three booleans: a
    /// context menu, a modal, a drag. A rising edge in any of them
    /// hands the floating layer to the window the update ran for.
    pub(crate) fn float_signature(&self) -> [bool; 3] {
        [
            self.overlay.is_some() || self.panels.burger_menu || self.card_context_menu.is_some(),
            self.error_dialog.is_some()
                || crate::state::Modal::ALL.iter().any(|&m| self.is_modal_open(m)),
            self.tab_drag.is_some() || self.card_drag.is_some(),
        ]
    }

    /// Give the floating layer to the current window when something
    /// just rose in it (see [`Self::float_signature`]).
    pub(crate) fn claim_floats(&mut self, before: [bool; 3]) {
        let after = self.float_signature();
        if before.iter().zip(after).any(|(b, a)| !*b && a) {
            self.float_window = self.window_ctx.as_ref().map(|c| c.id);
        }
    }

    /// Swap `id`'s values into the per-window fields. `false` when `id`
    /// is not an extra window, or one is already swapped in.
    fn enter_window(&mut self, id: window::Id) -> bool {
        if self.window_ctx.is_some() {
            return false;
        }
        let Some(w) = self.extra_windows.remove(&id) else {
            return false;
        };
        let main_active = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t._id);
        self.window_ctx = Some(WindowCtx {
            id,
            order: std::mem::replace(&mut self.tab_order, w.order),
            active: main_active,
            view: std::mem::replace(&mut self.active_view, View::Terminal),
            size: std::mem::replace(&mut self.window_size, w.size),
            cursor: std::mem::replace(&mut self.mouse_position, w.cursor),
            maximized: std::mem::replace(&mut self.window_maximized, w.maximized),
            fullscreen: std::mem::replace(&mut self.window_fullscreen, w.fullscreen),
            immersive: std::mem::replace(&mut self.fullscreen_immersive, w.immersive),
            focused: std::mem::replace(&mut self.window_focused, w.focused),
            awaiting: (w.awaiting_first_tab, w.born),
            mru: std::mem::replace(&mut self.tab_mru, w.mru),
        });
        self.active_tab = w
            .active
            .and_then(|a| self.tabs.iter().position(|t| t._id == a));
        true
    }

    /// Undo [`Self::enter_window`]: the extra window takes back whatever
    /// the handlers left in the fields, the main window gets its own
    /// back.
    fn exit_window(&mut self) {
        let Some(ctx) = self.window_ctx.take() else {
            return;
        };
        let prior = ctx.awaiting;
        let asked = (self.active_view != View::Terminal).then_some(self.active_view);
        let active = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t._id);
        let mut order = std::mem::replace(&mut self.tab_order, ctx.order);
        // An extra window holds terminal tabs only. A panel or SFTP
        // chip minted while its values were in the fields belongs to
        // the main strip.
        let mut strays = Vec::new();
        order.retain(|r| match r {
            TabRef::Terminal(_) => true,
            other => {
                strays.push(*other);
                false
            }
        });
        for r in strays {
            if !self.tab_order.contains(&r) {
                self.tab_order.push(r);
            }
        }
        let w = ExtraWindow {
            active: active.filter(|a| order.contains(&TabRef::Terminal(*a))).or_else(|| {
                order.iter().find_map(|r| match r {
                    TabRef::Terminal(id) => Some(*id),
                    _ => None,
                })
            }),
            order,
            size: std::mem::replace(&mut self.window_size, ctx.size),
            cursor: std::mem::replace(&mut self.mouse_position, ctx.cursor),
            maximized: std::mem::replace(&mut self.window_maximized, ctx.maximized),
            fullscreen: std::mem::replace(&mut self.window_fullscreen, ctx.fullscreen),
            immersive: std::mem::replace(&mut self.fullscreen_immersive, ctx.immersive),
            focused: std::mem::replace(&mut self.window_focused, ctx.focused),
            awaiting_first_tab: prior.0,
            born: prior.1,
            mru: std::mem::replace(&mut self.tab_mru, ctx.mru),
        };
        let mut w = w;
        if !w.order.is_empty() {
            w.awaiting_first_tab = false;
        }
        self.active_view = ctx.view;
        self.active_tab = ctx
            .active
            .and_then(|a| self.tabs.iter().position(|t| t._id == a));
        // A verb that belongs to the main window (Settings, a vault
        // view) was used from an extra one that still has tabs: it
        // happens in the main window, which comes forward. With no tab
        // left the view change is only the close path's "nothing to
        // show", and the window is on its way out.
        if let Some(view) = asked
            && !w.order.is_empty()
        {
            self.active_view = view;
            self.pending_focus_main = true;
        }
        self.extra_windows.insert(ctx.id, w);
    }

    /// Run `f` as window `target` (`None` = the main one), whichever
    /// window's values are in the fields right now, and put things back
    /// the way they were.
    pub(crate) fn as_window<R>(
        &mut self,
        target: Option<window::Id>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.window_ctx.as_ref().map(|c| c.id);
        if prev == target {
            return f(self);
        }
        if prev.is_some() {
            self.exit_window();
        }
        let entered = target.is_some_and(|t| self.enter_window(t));
        let out = f(self);
        if entered {
            self.exit_window();
        }
        if let Some(p) = prev {
            self.enter_window(p);
        }
        out
    }

    /// [`Self::as_window`] for a handler: the task it returns keeps the
    /// window too, so a follow-up that selects or closes "the active
    /// tab" still means that window's.
    pub(crate) fn run_in_window(
        &mut self,
        target: Option<window::Id>,
        f: impl FnOnce(&mut Self) -> Task<Message>,
    ) -> Task<Message> {
        let prev = self.window_ctx.as_ref().map(|c| c.id);
        let task = self.as_window(target, f);
        if prev == target {
            return task;
        }
        match target.or_else(crate::app::main_window_id) {
            Some(id) => task.map(move |m| Message::InWindow(id, Box::new(m))),
            None => task,
        }
    }

    /// A message that came from window `id`.
    pub(crate) fn update_in_window(&mut self, id: window::Id, message: Message) -> Task<Message> {
        let current = self.window_ctx.as_ref().map(|c| c.id);
        let target = if current == Some(id) || self.extra_windows.contains_key(&id) {
            Some(id)
        } else if current.is_some() && crate::app::main_window_id() == Some(id) {
            None
        } else {
            // The main window at the top level, or a window that is
            // gone: whatever is current handles it.
            return self.update(message);
        };
        let top_level = current.is_none();
        let task = self.run_in_window(target, |s| s.update(message));
        if !top_level {
            return task;
        }
        // The funnel `update` ends with ran nowhere yet (it stands down
        // while an extra window's values are in the fields): once, now,
        // as the main window.
        Task::batch([task, self.update(Message::NoOp)])
    }

    /// Every strip entry of every window: the main strip, then each
    /// extra window's. What "the open tabs" means to anything that
    /// outlives the windows (the restore-on-launch snapshot).
    pub(crate) fn all_strip_refs(&self) -> Vec<TabRef> {
        let mut refs = self.tab_order.clone();
        if let Some(ctx) = &self.window_ctx {
            refs.extend(ctx.order.iter().copied());
        }
        for w in self.extra_windows.values() {
            refs.extend(w.order.iter().copied());
        }
        refs
    }

    /// Terminal tabs that belong to a window OTHER than the one whose
    /// strip is in `tab_order`.
    pub(crate) fn tabs_in_other_windows(&self) -> std::collections::HashSet<Uuid> {
        let mut ids: std::collections::HashSet<Uuid> = self
            .extra_windows
            .values()
            .flat_map(|w| w.tab_ids())
            .collect();
        if let Some(ctx) = &self.window_ctx {
            ids.extend(ctx.order.iter().filter_map(|r| match r {
                TabRef::Terminal(id) => Some(*id),
                _ => None,
            }));
        }
        ids
    }

    /// Drop the refs of tabs that no longer exist from every strip that
    /// is not in `tab_order` (which `reconcile_tab_order` prunes itself).
    pub(crate) fn prune_other_window_strips(&mut self) {
        let alive: std::collections::HashSet<Uuid> = self.tabs.iter().map(|t| t._id).collect();
        for w in self.extra_windows.values_mut() {
            w.order
                .retain(|r| matches!(r, TabRef::Terminal(id) if alive.contains(id)));
            if w.active.is_none_or(|a| !alive.contains(&a)) {
                let first = w.tab_ids().next();
                w.active = first;
            }
        }
    }

    /// Hand a terminal tab's ref to the main strip while an extra
    /// window's values are in the fields, and make it the main window's
    /// active tab.
    pub(crate) fn park_ref_in_main(&mut self, id: Uuid, activate: bool) {
        if let Some(ctx) = self.window_ctx.as_mut() {
            if !ctx.order.contains(&TabRef::Terminal(id)) {
                ctx.order.push(TabRef::Terminal(id));
            }
            if activate {
                ctx.active = Some(id);
                ctx.view = View::Terminal;
            }
        }
    }

    /// What the windows themselves need after an update, as the main
    /// window: an extra window with no tab left closes, and a verb that
    /// belongs to the main window brings it forward.
    pub(crate) fn window_housekeeping(&mut self) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        if std::mem::take(&mut self.pending_focus_main) {
            tasks.push(crate::app::main_window().and_then(window::gain_focus));
        }
        if self.extra_windows.is_empty() {
            return tasks;
        }
        let dialing = self.connecting.is_some() || !self.batch_dials.is_empty();
        let dead: Vec<window::Id> = self
            .extra_windows
            .iter()
            .filter(|(_, w)| w.order.is_empty() && !(w.awaiting_first_tab && (dialing || !w.overdue())))
            .map(|(id, _)| *id)
            .collect();
        for id in dead {
            self.extra_windows.remove(&id);
            tasks.push(window::close(id));
        }
        tasks
    }
}
