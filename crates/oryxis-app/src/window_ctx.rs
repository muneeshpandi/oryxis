//! More than one window, one application.
//!
//! The app is an iced daemon: one process, one `Oryxis`, any number of
//! windows, and to the user none of them is special. Each is a whole
//! window: its own strip of tabs, its own screen (the hosts list, a
//! vault view, a tab), its own place in those screens. What they share
//! is everything that is the APP's: the vault and its data, the
//! settings, every session.
//!
//! Nothing about a tab lives in a window: every `TerminalTab` stays in
//! `Oryxis::tabs`, with its session, scrollback and panes, and a window
//! only says which of them it shows and in what order. Moving a tab
//! between windows therefore moves an id between two lists, and the
//! session never notices.
//!
//! What IS per window is small: the strip order, the active tab, the
//! view, the vault navigation ([`WindowNav`]), the size, the cursor, the
//! maximized / fullscreen flags. One window, the RESIDENT one, keeps
//! those in the `Oryxis` fields they always lived in; that is a storage
//! detail, not a role, and when the resident window closes another is
//! promoted into the fields ([`Oryxis::promote_window`]). Every other
//! window keeps its own copy in [`ExtraWindow`], and reaches the code
//! that reads those fields in two ways:
//!
//! - `update`: a message that comes from a window arrives as
//!   `Message::InWindow(id, ..)`, and [`Oryxis::update_in_window`] SWAPS
//!   that window's values into the fields for the duration, so every
//!   handler acts on "the active tab", "the strip" and "the view" of
//!   the window the user is in without knowing there is more than one.
//! - `view`: built from `&self`, where nothing can be swapped, so the
//!   window being drawn is ambient ([`viewing`]) and the views read the
//!   per-window values through the `cur_*` accessors below. During
//!   `update` nothing is being drawn and the accessors return the
//!   fields, which is what makes them safe to use anywhere.
//!
//! Things that exist ONCE are drawn in one window: a menu, a modal or a
//! drag in the window it was raised from (`float_window`), a side panel
//! in the window it was opened in (`panel_window`), a panel tab
//! (Settings) in the strip of the window that last asked for it.

use iced::Widget as _;
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

/// Where a window's user is in the vault screens: the open folder, the
/// searches and filters, the host selection. Each window has its own,
/// so two windows on the hosts list can sit in different folders with
/// different searches. The DATA those screens list is the app's and is
/// shared; so are the forms and the Settings panel, which exist once.
///
/// The resident window's lives in `Oryxis::nav`; it is swapped whole
/// with another window's in `enter_window` / `exit_window`, and a view
/// reads the one of the window being drawn through `Oryxis::cur_nav`.
#[derive(Debug, Clone, Default)]
pub(crate) struct WindowNav {
    /// What the cursor is over (cards, chips, rows), for the
    /// hover-revealed actions. Per window: the positions it names are
    /// positions in this window's lists, and a cursor is over one window.
    pub(crate) hover: crate::state::HoverState,
    pub(crate) active_group: Option<Uuid>,
    pub(crate) host_search: String,
    /// When set, the dashboard grid hides every host / group whose
    /// cloud origin doesn't match this profile id. Activated by
    /// clicking the small provider badge on a cloud-sourced host card,
    /// cleared from the chip at the top of the grid. None means no
    /// cloud filter.
    pub(crate) host_filter_cloud_profile: Option<Uuid>,
    /// Dashboard tag filter: only hosts carrying AT LEAST ONE of these
    /// tags (case-insensitive) are listed, and only groups whose
    /// subtree contains such a host render. In-memory like the search
    /// needle, not persisted; empty = no filter.
    pub(crate) host_filter_tags: Vec<String>,
    pub(crate) quick_host_input: String,
    /// Multi-selected host cards (issue #230): Ctrl / Shift + click, the
    /// hover check on a card, Space on a ringed card. Session-only.
    pub(crate) dash_selection: crate::state::DashSelection,
    /// The dashboard's multi-select mode (issue #230): while on, a click
    /// on a host card selects it instead of connecting, so a batch is
    /// built by pointing at cards rather than by holding Ctrl. A mode,
    /// not a modifier - toggled from the toolbar, left by Esc, and reset
    /// whenever the dashboard leaves the screen.
    pub(crate) dash_multi_select: bool,
    /// Workspace-mode contextual search backing for Snippets view.
    /// Matches against snippet label + command.
    pub(crate) snippet_search: String,
    /// Workspace-mode contextual search backing for History view.
    /// Matches against the connection label / hostname recorded in
    /// each log row.
    pub(crate) history_search: String,
    pub(crate) port_forward_search: String,
    /// Toolbar search needles for the Cloud Accounts and Proxies views.
    pub(crate) cloud_search: String,
    pub(crate) proxy_search: String,
    /// Vault Snippets view: the snippet group currently opened as a
    /// folder (dashboard-style drill-in). `None` = root (group cards +
    /// ungrouped snippets).
    pub(crate) active_snippet_group: Option<String>,
    /// Vault Snippets view: multi-select tag filter (in-memory, like
    /// the dashboard's `host_filter_tags`); empty = off.
    pub(crate) snippet_filter_tags: Vec<String>,
    /// History view host-tag filter (multi-select, matches the host
    /// tags of each row's connection), mirroring `host_filter_tags`;
    /// empty = off.
    pub(crate) history_filter_tags: Vec<String>,
}

/// What a window that is not the resident one keeps for itself.
#[derive(Debug, Clone)]
pub(crate) struct ExtraWindow {
    /// Its strip, in order.
    pub(crate) order: Vec<TabRef>,
    /// The screen it is on: the dashboard, a vault view, a panel, or a
    /// tab (`Terminal` / `Sftp`, named by `active`).
    pub(crate) view: View,
    /// The tab it shows, by strip id (`TerminalTab::_id` or
    /// `SftpTab::id`): an index would go stale the moment another
    /// window closed a tab.
    pub(crate) active: Option<Uuid>,
    /// Which surface of this window owns the live SFTP buffer while the
    /// window's values are in the fields. Parked back into the tab's
    /// own slot whenever they are not (see [`SftpOwner`]).
    pub(crate) sftp_owner: SftpOwner,
    pub(crate) size: Size,
    pub(crate) cursor: Point,
    pub(crate) maximized: bool,
    pub(crate) fullscreen: bool,
    pub(crate) immersive: bool,
    pub(crate) focused: bool,
    /// Its own most-recently-used order, so Ctrl+Tab cycles the tabs
    /// of the window it is pressed in.
    pub(crate) mru: Vec<TabRef>,
    /// Its place in the vault screens.
    pub(crate) nav: WindowNav,
}

impl ExtraWindow {
    pub(crate) fn new(order: Vec<TabRef>, view: View, size: Size) -> Self {
        let active = order.iter().find_map(|r| match r {
            TabRef::Terminal(id) | TabRef::Sftp(id) => Some(*id),
            TabRef::Panel(_) => None,
        });
        Self {
            order,
            view,
            active,
            sftp_owner: SftpOwner::None,
            size,
            cursor: Point::ORIGIN,
            maximized: false,
            fullscreen: false,
            immersive: false,
            focused: true,
            mru: Vec::new(),
            nav: WindowNav::default(),
        }
    }

    /// The strip ids of its tabs, terminal and SFTP.
    pub(crate) fn tab_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.order.iter().filter_map(|r| match r {
            TabRef::Terminal(id) | TabRef::Sftp(id) => Some(*id),
            TabRef::Panel(_) => None,
        })
    }
}

/// Who holds the live SFTP buffer (`Oryxis::sftp`).
///
/// The buffer is ONE: the active SFTP tab's state, or a terminal tab's
/// Files mode, is hoisted into it and every other surface keeps its
/// state in its own slot. With several windows that stays true by
/// making the buffer follow the window whose values are in the fields:
/// entering a window parks the previous owner into its slot and hoists
/// this window's, leaving it does the reverse. So a surface that is not
/// in the current window is always in its slot, which is where the
/// async routing (`route_sftp_async`) and an extra window's view
/// (`cur_sftp`) read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SftpOwner {
    #[default]
    None,
    /// A terminal tab in Files mode (`hybrid_sftp_owner`).
    Hybrid(Uuid),
    /// A standalone SFTP tab (`active_sftp`).
    Standalone(Uuid),
}

/// The resident window's own values, set aside while an extra window's are
/// in the fields.
#[derive(Debug)]
pub(crate) struct WindowCtx {
    pub(crate) id: window::Id,
    order: Vec<TabRef>,
    active: Option<Uuid>,
    view: View,
    sftp_owner: SftpOwner,
    size: Size,
    cursor: Point,
    maximized: bool,
    fullscreen: bool,
    immersive: bool,
    focused: bool,
    mru: Vec<TabRef>,
    nav: WindowNav,
}

impl Oryxis {
    /// The extra window being drawn, if the view under construction is
    /// one. Always `None` during `update`.
    fn viewed_extra(&self) -> Option<&ExtraWindow> {
        viewing().and_then(|id| self.extra_windows.get(&id))
    }

    /// The window a view or a handler is working for: the extra window
    /// being drawn, else the one whose values are swapped in, else
    /// `None` for the resident window.
    pub(crate) fn cur_window(&self) -> Option<window::Id> {
        viewing()
            .filter(|id| self.extra_windows.contains_key(id))
            .or(self.window_ctx.as_ref().map(|c| c.id))
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
        self.viewed_extra().map_or(self.active_view, |w| w.view)
    }

    /// The vault-screen navigation of the current window.
    pub(crate) fn cur_nav(&self) -> &WindowNav {
        self.viewed_extra().map_or(&self.nav, |w| &w.nav)
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
            None => crate::app::resident_window(),
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

    /// The live SFTP state of the current window: the buffer, except
    /// while an extra window is being drawn, whose surface is parked in
    /// its own slot (see [`SftpOwner`]).
    pub(crate) fn cur_sftp(&self) -> &crate::state::SftpState {
        let Some(w) = self.viewed_extra() else {
            return &self.sftp;
        };
        let Some(active) = w.active else {
            return &self.sftp_blank;
        };
        if let Some(tab) = self.sftp_tabs.iter().find(|t| t.id == active) {
            return &tab.state;
        }
        match self.tabs.iter().find(|t| t._id == active) {
            Some(tab) if tab.files_mode => &tab.files_state,
            _ => &self.sftp_blank,
        }
    }

    /// The terminal tab whose Files mode the current window shows.
    pub(crate) fn cur_hybrid_owner(&self) -> Option<Uuid> {
        match self.viewed_extra() {
            Some(w) => w.active.filter(|a| {
                self.tabs.iter().any(|t| t._id == *a && t.files_mode)
            }),
            None => self.hybrid_sftp_owner,
        }
    }

    /// The SFTP tab the current window shows, as an index.
    pub(crate) fn cur_active_sftp(&self) -> Option<usize> {
        match self.viewed_extra() {
            Some(w) => w
                .active
                .and_then(|a| self.sftp_tabs.iter().position(|t| t.id == a)),
            None => self.active_sftp,
        }
    }

    /// Whether the SFTP tab at `idx` is in the strip of the current
    /// window. Reads through `cur_tab_order`, so a menu being drawn and
    /// the handler it fires agree. Always true with a single window.
    pub(crate) fn shows_sftp_tab(&self, idx: usize) -> bool {
        if self.extra_windows.is_empty() && self.window_ctx.is_none() {
            return true;
        }
        self.sftp_tabs
            .get(idx)
            .is_some_and(|t| self.cur_tab_order().contains(&TabRef::Sftp(t.id)))
    }

    /// The window that shows the tab with strip id `id` (terminal or
    /// SFTP): `None` is the resident one.
    pub(crate) fn window_of_tab(&self, id: Uuid) -> Option<window::Id> {
        if let Some(ctx) = &self.window_ctx
            && self.tab_order.iter().any(|r| r.strip_id() == id)
        {
            return Some(ctx.id);
        }
        if self.extra_windows.is_empty() {
            return None;
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
                .map(move |m| Message::InWindow(id, Box::new(m))).boxed()
        } else {
            // Any other id is the resident window, which keeps the harness
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
            self.overlay.is_some()
                || self.panels.burger_menu
                || self.card_context_menu.is_some()
                || self.sftp.row_menu.is_some(),
            self.error_dialog.is_some()
                || crate::state::Modal::ALL.iter().any(|&m| self.is_modal_open(m)),
            self.tab_drag.is_some() || self.card_drag.is_some() || self.sftp.drag.is_some(),
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
        let resident_active = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t._id);
        let active_sftp = (w.view == View::Sftp)
            .then(|| w.active.and_then(|a| self.sftp_tabs.iter().position(|t| t.id == a)))
            .flatten();
        // The buffer changes hands: the resident window's surface goes
        // home to its slot, this window's comes in.
        let resident_sftp = self.park_sftp_owner();
        let owner = match (active_sftp, w.sftp_owner) {
            // It is on an SFTP tab, whatever was hoisted last.
            (Some(i), _) => SftpOwner::Standalone(self.sftp_tabs[i].id),
            (None, owner) => owner,
        };
        self.hoist_sftp_owner(owner);
        self.window_ctx = Some(WindowCtx {
            id,
            order: std::mem::replace(&mut self.tab_order, w.order),
            active: resident_active,
            view: std::mem::replace(&mut self.active_view, w.view),
            sftp_owner: resident_sftp,
            size: std::mem::replace(&mut self.window_size, w.size),
            cursor: std::mem::replace(&mut self.mouse_position, w.cursor),
            maximized: std::mem::replace(&mut self.window_maximized, w.maximized),
            fullscreen: std::mem::replace(&mut self.window_fullscreen, w.fullscreen),
            immersive: std::mem::replace(&mut self.fullscreen_immersive, w.immersive),
            focused: std::mem::replace(&mut self.window_focused, w.focused),
            mru: std::mem::replace(&mut self.tab_mru, w.mru),
            nav: std::mem::replace(&mut self.nav, w.nav),
        });
        self.active_tab = w
            .active
            .and_then(|a| self.tabs.iter().position(|t| t._id == a));
        true
    }

    /// Undo [`Self::enter_window`]: the extra window takes back whatever
    /// the handlers left in the fields, the resident window gets its own
    /// back.
    fn exit_window(&mut self) {
        let Some(ctx) = self.window_ctx.take() else {
            return;
        };
        let mut view = self.active_view;
        // The tab it shows: its SFTP tab on the SFTP surface, else its
        // terminal tab (none on the dashboard and the vault views).
        let active = if view == View::Sftp {
            self.active_sftp.and_then(|i| self.sftp_tabs.get(i)).map(|t| t.id)
        } else {
            self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t._id)
        };
        // The buffer goes back: this window's surface to its slot, the
        // resident window's in.
        let sftp_owner = self.park_sftp_owner();
        self.hoist_sftp_owner(ctx.sftp_owner);
        let order = std::mem::replace(&mut self.tab_order, ctx.order);
        let active = active.filter(|a| order.iter().any(|r| r.strip_id() == *a));
        // A tab surface with no tab behind it has nothing to draw.
        if active.is_none() && matches!(view, View::Terminal | View::Sftp) {
            view = View::Dashboard;
        }
        let w = ExtraWindow {
            active,
            view,
            sftp_owner,
            order,
            size: std::mem::replace(&mut self.window_size, ctx.size),
            cursor: std::mem::replace(&mut self.mouse_position, ctx.cursor),
            maximized: std::mem::replace(&mut self.window_maximized, ctx.maximized),
            fullscreen: std::mem::replace(&mut self.window_fullscreen, ctx.fullscreen),
            immersive: std::mem::replace(&mut self.fullscreen_immersive, ctx.immersive),
            focused: std::mem::replace(&mut self.window_focused, ctx.focused),
            mru: std::mem::replace(&mut self.tab_mru, ctx.mru),
            nav: std::mem::replace(&mut self.nav, ctx.nav),
        };
        self.active_view = ctx.view;
        self.active_tab = ctx
            .active
            .and_then(|a| self.tabs.iter().position(|t| t._id == a));
        self.extra_windows.insert(ctx.id, w);
    }

    /// Make the extra window `id` the RESIDENT one: its values move
    /// into the fields for good and whatever the fields held is
    /// dropped. Called when the resident window closes while others are
    /// open, with its strip already emptied. Returns the id of the
    /// window that was resident, for the caller to close.
    pub(crate) fn promote_window(&mut self, id: window::Id) -> Option<window::Id> {
        if self.window_ctx.is_some() || !self.extra_windows.contains_key(&id) {
            return None;
        }
        let old = crate::app::resident_window_id();
        self.enter_window(id);
        // What `enter_window` set aside is the closing window's state.
        self.window_ctx = None;
        crate::app::set_resident_window(id);
        // The geometry that is remembered is the resident's: start it
        // from where this window is now.
        self.window_windowed_size = self.window_size;
        self.window_windowed_pos = None;
        self.window_windowed_pos_prev = None;
        self.is_window_hidden = false;
        // Anything addressed to this window by id now means "resident".
        for slot in [&mut self.input_window, &mut self.float_window, &mut self.panel_window] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        self.batch_dial_windows.retain(|_, w| *w != id);
        old
    }

    /// How many windows are open.
    pub(crate) fn window_count(&self) -> usize {
        1 + self.extra_windows.len() + usize::from(self.window_ctx.is_some())
    }

    /// Every window but the current one (`None` is the resident), in a
    /// stable order.
    pub(crate) fn other_windows(&self) -> Vec<Option<window::Id>> {
        let current = self.cur_window();
        let mut out = Vec::new();
        if current.is_some() {
            out.push(None);
        }
        out.extend(
            self.extra_windows
                .keys()
                .filter(|id| Some(**id) != current)
                .map(|id| Some(*id)),
        );
        out
    }

    /// What a window is showing, as a short name for a menu: its active
    /// tab's label, else the hosts screen.
    pub(crate) fn window_label(&self, target: Option<window::Id>) -> String {
        let (view, active) = match target.and_then(|id| self.extra_windows.get(&id)) {
            Some(w) => (w.view, w.active),
            None => match &self.window_ctx {
                Some(ctx) => (ctx.view, ctx.active),
                None => (
                    self.active_view,
                    self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t._id),
                ),
            },
        };
        let tab = active.and_then(|a| {
            self.tabs
                .iter()
                .find(|t| t._id == a)
                .map(|t| t.label.clone())
                .or_else(|| self.sftp_tabs.iter().find(|t| t.id == a).map(|t| t.label.clone()))
        });
        match (view, tab) {
            (View::Terminal | View::Sftp, Some(label)) => label,
            (View::Sftp, None) => crate::i18n::t("sftp").to_string(),
            _ => crate::i18n::t("hosts").to_string(),
        }
    }

    /// The view a window is on (`None` is the resident one).
    fn view_of_window(&self, target: Option<window::Id>) -> View {
        match target.and_then(|id| self.extra_windows.get(&id)) {
            Some(w) => w.view,
            None => match &self.window_ctx {
                Some(ctx) if target.is_none() => ctx.view,
                _ => self.active_view,
            },
        }
    }

    /// Whether the side panel belongs to the current window. The window
    /// the user is working in has it whenever it is on a view the panel
    /// goes with (so the editor follows the user between two windows on
    /// the hosts screen); otherwise the window it was opened in keeps
    /// it.
    pub(crate) fn side_panel_here(&self) -> bool {
        let input = self.input_window();
        let owner = if self.side_panel_wanted_on(self.view_of_window(input)) {
            input
        } else {
            self.panel_window.filter(|w| {
                self.extra_windows.contains_key(w)
                    || self.window_ctx.as_ref().is_some_and(|c| c.id == *w)
            })
        };
        owner == self.cur_window()
    }

    /// A panel tab (Settings, the network tools) exists once, so opening
    /// it in the current window takes its chip out of every other strip,
    /// and a window that was showing it goes back to its hosts screen.
    pub(crate) fn take_panel_from_other_windows(&mut self, kind: crate::state::PanelKind) {
        let r = TabRef::Panel(kind);
        for w in self.extra_windows.values_mut() {
            w.order.retain(|x| *x != r);
            if w.view == kind.view() {
                w.view = View::Dashboard;
            }
        }
        if let Some(ctx) = self.window_ctx.as_mut() {
            ctx.order.retain(|x| *x != r);
            if ctx.view == kind.view() {
                ctx.view = View::Dashboard;
            }
        }
    }

    /// Run `f` as window `target` (`None` = the resident one), whichever
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
        match target.or_else(crate::app::resident_window_id) {
            Some(id) => task.map(move |m| Message::InWindow(id, Box::new(m))),
            None => task,
        }
    }

    /// A message that came from window `id`.
    pub(crate) fn update_in_window(&mut self, id: window::Id, message: Message) -> Task<Message> {
        let current = self.window_ctx.as_ref().map(|c| c.id);
        let target = if current == Some(id) || self.extra_windows.contains_key(&id) {
            Some(id)
        } else if current.is_some() && crate::app::resident_window_id() == Some(id) {
            None
        } else {
            // The resident window at the top level, or a window that is
            // gone. A gone window can still deliver a trailing frame
            // event (a resize, a focus loss), and that must not be read
            // as the current window's; everything else it "sends" is a
            // task that outlived it (a tab's output stream after the tab
            // moved on), which whatever is current handles. The harness
            // emulator keeps drawing under the id of a window that
            // closed, so there every id that is not a window is the
            // resident one.
            if crate::app::resident_window_id().is_some_and(|r| r != id)
                && !crate::app::harness_active()
                && is_frame_event(&message)
            {
                return Task::none();
            }
            return self.update(message);
        };
        let top_level = current.is_none();
        let task = self.run_in_window(target, |s| s.update(message));
        if !top_level {
            return task;
        }
        // The funnel `update` ends with ran nowhere yet (it stands down
        // while an extra window's values are in the fields): once, now,
        // as the resident window.
        Task::batch([task, self.update(Message::NoOp)])
    }

    /// Every strip entry of every window: the resident strip, then each
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

    /// Strip ids of the tabs (terminal and SFTP) that belong to a window
    /// OTHER than the one whose strip is in `tab_order`.
    pub(crate) fn tabs_in_other_windows(&self) -> std::collections::HashSet<Uuid> {
        let mut ids: std::collections::HashSet<Uuid> = self
            .extra_windows
            .values()
            .flat_map(|w| w.tab_ids())
            .collect();
        if let Some(ctx) = &self.window_ctx {
            ids.extend(ctx.order.iter().filter_map(|r| match r {
                TabRef::Terminal(id) | TabRef::Sftp(id) => Some(*id),
                TabRef::Panel(_) => None,
            }));
        }
        ids
    }

    /// Drop the refs of tabs that no longer exist from every strip that
    /// is not in `tab_order` (which `reconcile_tab_order` prunes itself).
    pub(crate) fn prune_other_window_strips(&mut self) {
        let alive: std::collections::HashSet<Uuid> = self
            .tabs
            .iter()
            .map(|t| t._id)
            .chain(self.sftp_tabs.iter().map(|t| t.id))
            .collect();
        let panels = &self.open_panel_tabs;
        for w in self.extra_windows.values_mut() {
            w.order.retain(|r| match r {
                TabRef::Terminal(id) | TabRef::Sftp(id) => alive.contains(id),
                TabRef::Panel(kind) => panels.contains(kind),
            });
            if w.active.is_some_and(|a| !alive.contains(&a)) {
                w.active = None;
            }
            // A window left on a surface whose tab or panel is gone goes
            // back to its hosts screen.
            let orphaned = match w.view {
                View::Terminal | View::Sftp => w.active.is_none(),
                other => crate::state::PanelKind::for_view(other)
                    .is_some_and(|kind| !panels.contains(&kind)),
            };
            if orphaned {
                w.view = View::Dashboard;
            }
        }
    }

    /// Hand a tab's ref to another window's strip (`None` is the
    /// resident one), optionally making it that window's active tab.
    /// Called with the SOURCE window current, after the ref left its
    /// strip.
    pub(crate) fn give_ref_to_window(
        &mut self,
        target: Option<window::Id>,
        r: TabRef,
        activate: bool,
    ) {
        let (order, active, view, sftp_owner) = match target {
            Some(id) => match self.extra_windows.get_mut(&id) {
                Some(w) => (&mut w.order, &mut w.active, &mut w.view, &mut w.sftp_owner),
                None => return,
            },
            None => match self.window_ctx.as_mut() {
                Some(ctx) => (&mut ctx.order, &mut ctx.active, &mut ctx.view, &mut ctx.sftp_owner),
                // The resident window is the current one.
                None => return,
            },
        };
        if !order.contains(&r) {
            order.push(r);
        }
        if !activate {
            return;
        }
        match r {
            TabRef::Terminal(id) => {
                *active = Some(id);
                *view = View::Terminal;
            }
            TabRef::Sftp(id) => {
                // An extra window names its SFTP tab in `active`; the
                // resident's parked `active` is its TERMINAL tab, and
                // the SFTP surface is up when it has none.
                *active = target.map(|_| id);
                *view = View::Sftp;
                *sftp_owner = SftpOwner::Standalone(id);
            }
            TabRef::Panel(_) => {}
        }
    }

    /// Send whichever surface holds the live SFTP buffer home to its
    /// own slot, and say who it was. Storage only: nothing about the
    /// surface changes, so none of the bookkeeping a real focus change
    /// does (the slow-click rename generation) applies.
    fn park_sftp_owner(&mut self) -> SftpOwner {
        if let Some(id) = self.hybrid_sftp_owner.take() {
            if let Some(tab) = self.tabs.iter_mut().find(|t| t._id == id) {
                *tab.files_state = std::mem::take(&mut self.sftp);
                return SftpOwner::Hybrid(id);
            }
            self.sftp = crate::state::SftpState::default();
            return SftpOwner::None;
        }
        if let Some(idx) = self.active_sftp.take()
            && let Some(tab) = self.sftp_tabs.get_mut(idx)
        {
            tab.state = std::mem::take(&mut self.sftp);
            return SftpOwner::Standalone(tab.id);
        }
        SftpOwner::None
    }

    /// The reverse of [`Self::park_sftp_owner`]. Expects the buffer
    /// unowned. An owner that is gone (its tab closed meanwhile) leaves
    /// it that way.
    fn hoist_sftp_owner(&mut self, owner: SftpOwner) {
        match owner {
            SftpOwner::None => {}
            SftpOwner::Hybrid(id) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t._id == id) {
                    self.sftp = std::mem::take(&mut *tab.files_state);
                    self.hybrid_sftp_owner = Some(id);
                }
            }
            SftpOwner::Standalone(id) => {
                if let Some(idx) = self.sftp_tabs.iter().position(|t| t.id == id) {
                    self.sftp = std::mem::take(&mut self.sftp_tabs[idx].state);
                    self.active_sftp = Some(idx);
                }
            }
        }
    }

    /// What the windows themselves need after an update, run with the
    /// resident window current: windows the update asked to close are
    /// closed, and the window a tab was moved into comes forward.
    pub(crate) fn window_housekeeping(&mut self) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        for id in std::mem::take(&mut self.windows_closing) {
            if self.extra_windows.remove(&id).is_some() {
                tasks.push(window::close(id));
            }
        }
        if let Some(target) = self.pending_focus.take()
            && let Some(id) = target
                .filter(|id| self.extra_windows.contains_key(id))
                .or_else(crate::app::resident_window_id)
        {
            tasks.push(window::gain_focus(id));
        }
        tasks
    }
}

/// The messages the event listener produces about a window's own frame
/// and pointer.
fn is_frame_event(message: &Message) -> bool {
    use crate::app::TabsMessage as T;
    matches!(
        message,
        Message::Tabs(
            T::WindowResized(_)
                | T::WindowMoved(_)
                | T::WindowFocusChanged(_)
                | T::WindowStateSynced { .. }
                | T::WindowFullscreenSettled(_)
                | T::MouseMoved(_)
        )
    )
}
