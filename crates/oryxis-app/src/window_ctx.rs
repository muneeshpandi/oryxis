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
    /// The vault of the extra window being drawn (`VaultSlots::cur`).
    static VIEWED_VAULT: Cell<Option<Uuid>> = const { Cell::new(None) };
}

/// The vault of the window whose view is being built, when that is an
/// extra window. `None` during `update` and while the resident window is
/// drawn: then the vault is the one in the fields
/// (`VaultSlots::current_id`).
pub(crate) fn viewed_vault() -> Option<Uuid> {
    VIEWED_VAULT.with(Cell::get)
}

/// Run `f` as if the view of an extra window on `vault` were being built.
#[cfg(test)]
pub(crate) fn with_viewed_vault_for_test<R>(vault: Option<Uuid>, f: impl FnOnce() -> R) -> R {
    let _pass = ViewPass::begin(Viewing::Nothing, vault);
    f()
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
    fn begin(target: Viewing, vault: Option<Uuid>) -> Self {
        VIEWING.with(|v| v.set(target));
        VIEWED_VAULT.with(|v| v.set(vault));
        Self
    }
}

impl Drop for ViewPass {
    fn drop(&mut self) {
        VIEWING.with(|v| v.set(Viewing::Nothing));
        VIEWED_VAULT.with(|v| v.set(None));
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
    /// Judges each left press on a terminal tab chip against the previous
    /// one, keyed by the tab's id (`tab_double_click`, issue #277). Per
    /// window like the strip it watches.
    pub(crate) tab_click: crate::double_click::ClickTracker<Uuid>,
    /// What the last chip press turned out to be, waiting for that chip's
    /// own release message (`TabChipClicked`), which takes it.
    pub(crate) tab_press_verdict: Option<(Uuid, crate::double_click::ClickReport)>,
}

/// What a window keeps of a vault it is not working on right now: the
/// tabs of that vault it showed, the one in front, its MRU, the screen it
/// was on and its place in the vault screens. Restored when the window
/// switches back (`switch_window_vault`); its tabs keep running hidden
/// meanwhile. Panel tabs (Settings, network tools) are not parked: they
/// follow the window to the vault it switches to.
#[derive(Debug, Clone)]
pub(crate) struct ParkedView {
    pub(crate) order: Vec<TabRef>,
    pub(crate) active: Option<Uuid>,
    pub(crate) mru: Vec<TabRef>,
    pub(crate) view: View,
    pub(crate) nav: WindowNav,
}

impl ParkedView {
    fn tab_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.order.iter().filter_map(|r| match r {
            TabRef::Terminal(id) | TabRef::Sftp(id) => Some(*id),
            TabRef::Panel(_) => None,
        })
    }
}

/// A window's parked views, by vault.
pub(crate) type ParkedViews = std::collections::HashMap<Uuid, ParkedView>;

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
    /// The vault it works on (`vault_slots`).
    pub(crate) vault: Uuid,
    /// What it keeps of the other vaults it worked on.
    pub(crate) parked: ParkedViews,
}

impl ExtraWindow {
    pub(crate) fn new(order: Vec<TabRef>, view: View, size: Size, vault: Uuid) -> Self {
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
            vault,
            parked: ParkedViews::new(),
        }
    }

    /// The strip ids of its tabs, terminal and SFTP, shown or parked.
    pub(crate) fn tab_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.order
            .iter()
            .filter_map(|r| match r {
                TabRef::Terminal(id) | TabRef::Sftp(id) => Some(*id),
                TabRef::Panel(_) => None,
            })
            .chain(self.parked.values().flat_map(ParkedView::tab_ids))
    }
}

/// A window being carried by the mouse, with the button still down:
/// the one a tab was just dragged out into, or the window itself when
/// the dragged tab was the only one in it.
///
/// The button was pressed in the HOLDER, and the system keeps
/// delivering the pointer to the window a press began in, so the holder
/// goes on receiving the moves and, in the end, the release, in ITS
/// coordinates. Its position on the screen turns those into screen
/// coordinates. On each move the cursor is tested against the tab
/// strips of the other windows: over one, the carried window is hidden
/// and that strip shows the tab as a ghost at the slot it would take
/// (`hover`); anywhere else the carried window is shown and put under
/// the cursor. The release over a strip docks the tab at that slot.
///
/// That needs window positions, which Wayland does not give. There the
/// compositor carries the window itself when it offers
/// `xdg-toplevel-drag` (`native`); without the protocol no carry starts
/// and the new window stays where the compositor put it.
#[derive(Debug, Clone)]
pub(crate) struct WindowCarry {
    /// The window being carried.
    pub(crate) window: window::Id,
    /// The tab it carries.
    pub(crate) tab: Uuid,
    /// The window the button is held in (`None` is the resident one).
    /// It is also the carried window when the tab was the only one in
    /// it.
    pub(crate) holder: Option<window::Id>,
    /// The holder's position on the screen. Follows the moves when the
    /// holder is the window being carried.
    pub(crate) origin: Point,
    /// Where the carried window's corner sits relative to the cursor.
    pub(crate) grab: Point,
    /// The carried window is hidden: the cursor is over another
    /// window's strip, which shows the tab instead.
    pub(crate) hidden: bool,
    /// The screen rectangle of every OTHER window (the carried one
    /// excluded), as measured when the carry began: where it can dock.
    pub(crate) docks: Vec<(Option<window::Id>, iced::Rectangle)>,
    /// The COMPOSITOR is carrying the window (Wayland's
    /// `xdg-toplevel-drag`, `window::drag_toplevel`) instead of the app
    /// moving it. The holder then receives no pointer at all for the
    /// length of the drag, and the windows under the cursor report it
    /// themselves (`hover`).
    pub(crate) native: bool,
    /// The strip the tab is over right now: the window (`None` is the
    /// resident one), the cursor's position in it, and the slot of
    /// that window's strip order the tab would take.
    pub(crate) hover: Option<CarryHover>,
}

/// A carried tab over another window's strip (see [`WindowCarry`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CarryHover {
    pub(crate) window: Option<window::Id>,
    pub(crate) cursor: Point,
    pub(crate) slot: usize,
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
    vault: crate::vault_slots::SlotMark,
    parked: ParkedViews,
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

    /// The context menu, when it belongs to the current window. Views
    /// style the control that opened it as open, and only the window it
    /// was raised from has one.
    pub(crate) fn cur_overlay(&self) -> Option<&crate::state::OverlayState> {
        self.overlay.as_ref().filter(|_| self.floats_here())
    }

    /// The host card whose context menu is up, when it is up here.
    pub(crate) fn cur_card_context_menu(&self) -> Option<Uuid> {
        self.card_context_menu.filter(|_| self.floats_here())
    }

    /// Whether the burger menu is open in the current window.
    pub(crate) fn cur_burger_open(&self) -> bool {
        self.vaults.cur().panels.burger_menu && self.floats_here()
    }

    /// A carried tab hovering the current window's strip: the tab, the
    /// cursor in this window's coordinates, and the slot it would take.
    pub(crate) fn carry_hover_here(&self) -> Option<(Uuid, CarryHover)> {
        let carry = self.window_carry.as_ref()?;
        let hover = carry.hover?;
        (hover.window == self.cur_window()).then_some((carry.tab, hover))
    }

    /// The tab being dragged over the current window's strip: a reorder
    /// that began here, or a carried tab from another window. The strip
    /// leaves its gap and draws its ghost for it.
    pub(crate) fn strip_drag_id(&self) -> Option<Uuid> {
        if let Some((tab, _)) = self.carry_hover_here() {
            return Some(tab);
        }
        self.tab_drag
            .filter(|d| d.active && self.floats_here())
            .map(|d| d.from_id)
    }

    /// Where the strip's drag ghost follows: the carried tab's cursor
    /// while one hovers here (this window does not get the pointer, the
    /// holder does), else this window's own cursor.
    pub(crate) fn strip_cursor(&self) -> Point {
        self.carry_hover_here()
            .map(|(_, hover)| hover.cursor)
            .unwrap_or_else(|| self.cur_mouse())
    }

    /// Whether the terminal tab at `idx` is in the strip of the window
    /// the handler is running for. Always true with a single window.
    pub(crate) fn shows_tab(&self, idx: usize) -> bool {
        if self.extra_windows.is_empty() && self.window_ctx.is_none() && self.parked_views.is_empty() {
            return true;
        }
        self.tabs
            .get(idx)
            .is_some_and(|t| self.cur_tab_order().contains(&TabRef::Terminal(t._id)))
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

    /// The dual-pane SFTP surface the current window shows, by tab id: its
    /// terminal tab in Files mode, else its SFTP tab. The view's
    /// counterpart of `current_sftp_owner`, which reads the FIELDS (the
    /// window whose update runs, or the resident one while a view is
    /// built) and so names another window's surface in a second window.
    pub(crate) fn cur_sftp_owner(&self) -> Option<Uuid> {
        self.cur_hybrid_owner()
            .filter(|id| self.tabs.iter().any(|t| t._id == *id))
            .or_else(|| self.cur_active_sftp().and_then(|i| self.sftp_tabs.get(i)).map(|t| t.id))
    }

    /// Whether the SFTP tab at `idx` is in the strip of the current
    /// window. Reads through `cur_tab_order`, so a menu being drawn and
    /// the handler it fires agree. Always true with a single window.
    pub(crate) fn shows_sftp_tab(&self, idx: usize) -> bool {
        if self.extra_windows.is_empty() && self.window_ctx.is_none() && self.parked_views.is_empty() {
            return true;
        }
        self.sftp_tabs
            .get(idx)
            .is_some_and(|t| self.cur_tab_order().contains(&TabRef::Sftp(t.id)))
    }

    /// The window that shows the tab holding `pane`, the resident one by
    /// its id; `None` when no tab holds the pane any more.
    pub(crate) fn window_of_pane(&self, pane: Uuid) -> Option<window::Id> {
        let tab = self.pane_tab_index(pane).map(|i| self.tabs[i]._id)?;
        self.window_of_tab(tab).or_else(crate::app::resident_window_id)
    }

    /// `Message::ForPane` arriving with no window: run it in the window
    /// that shows the pane's tab (the resident included, so its widget
    /// operations stay there), or as the plain message when the pane is
    /// gone.
    pub(crate) fn update_for_pane(&mut self, pane: Uuid, message: Message) -> Task<Message> {
        let message = self.on_vault_of_pane(pane, message);
        match self.window_of_pane(pane) {
            Some(target) => self.update_in_window(target, message),
            None => self.update(message),
        }
    }

    /// The vault a tab belongs to (`TerminalTab::vault` / `SftpTab::vault`).
    pub(crate) fn vault_of_tab(&self, id: Uuid) -> Option<Uuid> {
        self.tabs
            .iter()
            .find(|t| t._id == id)
            .map(|t| t.vault)
            .or_else(|| self.sftp_tabs.iter().find(|t| t.id == id).map(|t| t.vault))
            .flatten()
    }

    /// `message` about `pane`, to run on the vault of the pane's tab: the
    /// window holding the tab may be working on another vault while the
    /// tab is parked there (`ParkedView`).
    fn on_vault_of_pane(&self, pane: Uuid, message: Message) -> Message {
        let vault = self
            .pane_tab_index(pane)
            .and_then(|i| self.tabs[i].vault);
        match vault {
            Some(vault) => Message::ForVault(vault, Box::new(message)),
            None => message,
        }
    }

    /// Run `message` with vault `vault` as the one `update` works on,
    /// whatever window's values are in the fields, and keep its follow-ups
    /// on that vault. A vault that is not open runs the message as is.
    pub(crate) fn update_on_vault(&mut self, vault: Uuid, message: Message) -> Task<Message> {
        if self.vaults.current_id() == vault {
            return self.update(message);
        }
        // A follow-up of a vault closed since (removed from the list,
        // deleted, its setup cancelled) belongs to nothing that is left;
        // run on another vault, it would write into that vault.
        if !self.vaults.is_open(vault) {
            tracing::warn!(%vault, "dropped a follow-up of a closed vault");
            return Task::none();
        }
        let previous = self.vaults.act_on(vault);
        // `update` keeps the follow-ups on `vault`.
        let task = self.update(message);
        self.vaults.act_back(previous);
        task
    }

    /// Point the window in the fields at vault `target` (the switcher):
    /// what it shows of the vault it leaves is parked, panels aside (they
    /// follow it: Settings now edits `target`), and what it kept of
    /// `target` comes back; the hosts screen the first time. The tabs it
    /// parks keep running.
    pub(crate) fn switch_window_vault(&mut self, target: Uuid) {
        let from = self.vaults.window_id();
        if from == target || !self.vaults.is_open(target) {
            return;
        }
        let on_panel = crate::state::PanelKind::for_view(self.active_view).is_some();
        // The SFTP buffer goes home to its tab before the tab is parked.
        let owner = self.park_sftp_owner();
        let active = match (self.active_view, owner) {
            (View::Sftp, SftpOwner::Standalone(id)) => Some(id),
            _ => self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t._id),
        };
        let is_panel = |r: &TabRef| matches!(r, TabRef::Panel(_));
        let (panels, tabs): (Vec<TabRef>, Vec<TabRef>) =
            std::mem::take(&mut self.tab_order).into_iter().partition(is_panel);
        let (panel_mru, tab_mru): (Vec<TabRef>, Vec<TabRef>) =
            std::mem::take(&mut self.tab_mru).into_iter().partition(is_panel);
        let left = ParkedView {
            order: tabs,
            active,
            mru: tab_mru,
            view: if on_panel { View::Dashboard } else { self.active_view },
            nav: std::mem::take(&mut self.nav),
        };
        self.parked_views.insert(from, left);
        // A menu, a card context menu or a drag raised over the vault
        // this window leaves is index-keyed into ITS lists; over the
        // other vault's they would act on the wrong row.
        if self.floats_here() {
            self.overlay = None;
            self.card_context_menu = None;
        }
        self.on_window_blur();
        self.vaults.switch_window_to(target);
        // Brought up by the user: its idle clock starts now, or a vault
        // left in the background for longer than its threshold would
        // lock under the user at the next tick.
        self.vaults.cur_mut().last_user_activity = std::time::Instant::now();
        let back = self.parked_views.remove(&target).unwrap_or(ParkedView {
            order: Vec::new(),
            active: None,
            mru: Vec::new(),
            view: View::Dashboard,
            nav: WindowNav::default(),
        });
        self.tab_order = back.order;
        self.tab_order.extend(panels);
        self.tab_mru = back.mru;
        self.tab_mru.extend(panel_mru);
        self.nav = back.nav;
        self.active_tab = None;
        let mut view = back.view;
        match (view, back.active) {
            (View::Sftp, Some(id)) => {
                self.hoist_sftp_owner(SftpOwner::Standalone(id));
                if self.active_sftp.is_none() {
                    view = View::Dashboard;
                }
            }
            (_, Some(id)) => {
                self.active_tab = self.tabs.iter().position(|t| t._id == id);
                if view == View::Terminal && self.active_tab.is_none() {
                    view = View::Dashboard;
                }
            }
            (View::Terminal | View::Sftp, None) => view = View::Dashboard,
            _ => {}
        }
        if !on_panel {
            self.active_view = view;
        }
        // Sync the process-wide tab-bar-position atomic to the vault we
        // just switched to. It is otherwise only set on a vault's FIRST
        // load (`load_vault_terminal_settings`), so switching to an
        // already-loaded vault left the atomic on the PREVIOUS vault's
        // position. That desync is what made the vault switcher vanish:
        // `build_base` reads the per-vault `tab_bar_position` (e.g.
        // "left") and draws the slim chrome bar, but that bar pushes the
        // Home/vault chip only when `tab_bar_pos()` (the stale atomic,
        // still "top") reports a side dock — so the chip was skipped and
        // there was no way to switch vaults again. Same atomic drives
        // tab sizing / drag bands, so keeping it in step fixes those too.
        crate::views::tab_bar::set_tab_bar_pos(
            crate::views::tab_bar::TabBarPos::from_setting(
                &self.vaults.cur().prefs.tab_bar_position,
            ),
        );
    }

    /// The window that shows the tab with strip id `id` (terminal or
    /// SFTP): `None` is the resident one.
    pub(crate) fn window_of_tab(&self, id: Uuid) -> Option<window::Id> {
        if let Some(ctx) = &self.window_ctx
            && (self.tab_order.iter().any(|r| r.strip_id() == id)
                || self.tabs_parked_here().contains(&id))
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
        if let Some(window) = self.extra_windows.get(&id) {
            let _pass = ViewPass::begin(Viewing::Extra(id), Some(window.vault));
            // The window builds and draws in its own vault's theme.
            let theme = self.vaults.cur().ui_theme;
            let _scope = crate::theme::SelectionScope::enter(theme);
            crate::widgets::theme_scope(
                self.view().map(move |m| Message::InWindow(id, Box::new(m))).boxed(),
                theme,
            )
        } else {
            // Any other id is the resident window, which keeps the harness
            // (its emulator owns a single window) and a window on its
            // way out on the ordinary path.
            let _pass = ViewPass::begin(Viewing::Main, None);
            // The start picker is in no vault's theme: none is chosen yet.
            let theme = if self.start_picker {
                crate::theme::ThemeSelection::named(None)
            } else {
                self.vaults.cur().ui_theme
            };
            let _scope = crate::theme::SelectionScope::enter(theme);
            crate::widgets::theme_scope(self.view(), theme)
        }
    }

    /// The theme window `id` draws in: its vault's (the resident window's
    /// for any id that is not an extra window). Read by
    /// `Oryxis::theme_for`, which iced asks per window.
    pub(crate) fn theme_of_window(&self, id: window::Id) -> crate::theme::ThemeSelection {
        let vault = self.extra_windows.get(&id).map(|w| w.vault);
        if vault.is_none() && self.start_picker {
            return crate::theme::ThemeSelection::named(None);
        }
        let _pass = ViewPass::begin(Viewing::Nothing, vault);
        self.vaults.cur().ui_theme
    }

    /// The window the user is working in (the focused one), when it is
    /// an extra window that still exists.
    pub(crate) fn input_window(&self) -> Option<window::Id> {
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
                || self.vaults.cur().panels.burger_menu
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
            vault: self.vaults.enter_window(w.vault),
            parked: std::mem::replace(&mut self.parked_views, w.parked),
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
            vault: self.vaults.leave_window(ctx.vault),
            parked: std::mem::replace(&mut self.parked_views, ctx.parked),
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
        // from where this window is now, unless that is a maximized or
        // fullscreen rectangle (the monitor's, never a windowed size).
        if !self.window_maximized && !self.window_fullscreen {
            self.window_windowed_size = self.window_size;
        }
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

    /// The size of a window (`None` is the resident one), whichever
    /// window's values are in the fields.
    pub(crate) fn size_of_window(&self, target: Option<window::Id>) -> Size {
        match target.and_then(|id| self.extra_windows.get(&id)) {
            Some(w) => w.size,
            None => match &self.window_ctx {
                Some(ctx) if target.is_none() => ctx.size,
                _ => self.window_size,
            },
        }
    }

    /// Every open window, as the app names them: the resident one is
    /// `None`. Paired with its window id when that is known.
    pub(crate) fn all_windows(&self) -> Vec<(Option<window::Id>, window::Id)> {
        let mut out = Vec::new();
        if let Some(id) = crate::app::resident_window_id() {
            out.push((None, id));
        }
        if let Some(ctx) = &self.window_ctx {
            out.push((Some(ctx.id), ctx.id));
        }
        out.extend(self.extra_windows.keys().map(|id| (Some(*id), *id)));
        out
    }

    /// How many windows are open.
    pub(crate) fn window_count(&self) -> usize {
        1 + self.extra_windows.len() + usize::from(self.window_ctx.is_some())
    }

    /// The vault window `target` works on (`None` = the resident one),
    /// whichever window's values are in the fields.
    pub(crate) fn vault_of_window(&self, target: Option<window::Id>) -> Uuid {
        match target {
            Some(id) if self.window_ctx.as_ref().is_some_and(|c| c.id == id) => self.vaults.window_id(),
            Some(id) => match self.extra_windows.get(&id) {
                Some(w) => w.vault,
                None => self.vaults.window_id(),
            },
            None => match &self.window_ctx {
                Some(ctx) => ctx.vault.window(),
                None => self.vaults.window_id(),
            },
        }
    }

    /// A window working on vault `vault`: `Some(None)` for the resident
    /// one (preferred), `Some(Some(id))` for an extra one, `None` when no
    /// window shows it.
    pub(crate) fn window_on_vault(&self, vault: Uuid) -> Option<Option<window::Id>> {
        if self.vault_of_window(None) == vault {
            return Some(None);
        }
        self.extra_windows
            .iter()
            .find(|(_, w)| w.vault == vault)
            .map(|(id, _)| Some(*id))
            .or_else(|| {
                // The window in the fields, when it is an extra one.
                self.window_ctx
                    .as_ref()
                    .filter(|_| self.vaults.window_id() == vault)
                    .map(|c| Some(c.id))
            })
    }

    /// Every window working on vault `vault` (`None` = the resident one).
    pub(crate) fn windows_on_vault(&self, vault: Uuid) -> Vec<Option<window::Id>> {
        let mut out: Vec<Option<window::Id>> = Vec::new();
        if self.vault_of_window(None) == vault {
            out.push(None);
        }
        if let Some(ctx) = &self.window_ctx
            && self.vaults.window_id() == vault
        {
            out.push(Some(ctx.id));
        }
        out.extend(self.extra_windows.iter().filter(|(_, w)| w.vault == vault).map(|(id, _)| Some(*id)));
        out
    }

    /// Forget every view of vault `vault` a window parked (the vault is
    /// going away; its tabs already ended).
    pub(crate) fn drop_parked_views_of(&mut self, vault: Uuid) {
        self.parked_views.remove(&vault);
        if let Some(ctx) = self.window_ctx.as_mut() {
            ctx.parked.remove(&vault);
        }
        for w in self.extra_windows.values_mut() {
            w.parked.remove(&vault);
        }
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
        // Its follow-ups run on the vault `f` worked on (the target
        // window's), read before the window leaves the fields: the
        // outer `update` tags with the vault acting when IT ends, the
        // originating window's, and a vault's first showing in a new
        // window (`vault_shown` -> `finish_vault_open`) would have its
        // engine, forwards and plugin answers land on that one.
        let mut vault = None;
        let task = self.as_window(target, |app| {
            let task = f(app);
            vault = Some(app.vaults.current_id());
            task
        });
        let task = match vault {
            Some(vault) => task.map(move |m| m.on_vault(vault)),
            None => task,
        };
        if prev == target {
            return task;
        }
        // Its widget operations (a focus, a scroll) are this window's
        // too: by default the runtime runs them in every window, and
        // two windows on the same screen share every widget id.
        match target.or_else(crate::app::resident_window_id) {
            Some(id) => task.map(move |m| Message::InWindow(id, Box::new(m))).in_window(id),
            None => task,
        }
    }

    /// A message that came from window `id`.
    pub(crate) fn update_in_window(&mut self, id: window::Id, message: Message) -> Task<Message> {
        // A message about a pane runs where the pane's tab is NOW, not
        // where its stream was started. Resolved before any swap: once a
        // window's values are in the fields, another window cannot be
        // entered.
        if let Message::ForPane(pane, inner) = message {
            let target = self.window_of_pane(pane).unwrap_or(id);
            let inner = self.on_vault_of_pane(pane, *inner);
            return self.update_in_window(target, inner);
        }
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
            let task = self.update(message);
            // The resident window's own message: its widget operations
            // stay in it.
            return match crate::app::resident_window_id() {
                Some(resident) if resident == id => task.in_window(id),
                _ => task,
            };
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
        refs.extend(self.parked_views.values().flat_map(|p| p.order.iter().copied()));
        if let Some(ctx) = &self.window_ctx {
            refs.extend(ctx.order.iter().copied());
            refs.extend(ctx.parked.values().flat_map(|p| p.order.iter().copied()));
        }
        for w in self.extra_windows.values() {
            refs.extend(w.order.iter().copied());
            refs.extend(w.parked.values().flat_map(|p| p.order.iter().copied()));
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
            ids.extend(ctx.parked.values().flat_map(ParkedView::tab_ids));
        }
        ids
    }

    /// The tabs the window in the fields keeps parked under the vaults it
    /// is not working on: held by it, not shown.
    pub(crate) fn tabs_parked_here(&self) -> std::collections::HashSet<Uuid> {
        self.parked_views.values().flat_map(ParkedView::tab_ids).collect()
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
        // Parked views hold tabs only (panels follow the window).
        let prune_parked = |parked: &mut ParkedViews| {
            for p in parked.values_mut() {
                p.order.retain(|r| r.tab_id_in(&alive));
                p.mru.retain(|r| r.tab_id_in(&alive));
                if p.active.is_some_and(|a| !alive.contains(&a)) {
                    p.active = None;
                    if matches!(p.view, View::Terminal | View::Sftp) {
                        p.view = View::Dashboard;
                    }
                }
            }
        };
        prune_parked(&mut self.parked_views);
        if let Some(ctx) = self.window_ctx.as_mut() {
            prune_parked(&mut ctx.parked);
        }
        for w in self.extra_windows.values_mut() {
            prune_parked(&mut w.parked);
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

    /// The strip ref of a terminal or SFTP tab, wherever it is shown.
    pub(crate) fn ref_of_tab(&self, id: Uuid) -> Option<TabRef> {
        if self.tabs.iter().any(|t| t._id == id) {
            Some(TabRef::Terminal(id))
        } else if self.sftp_tabs.iter().any(|t| t.id == id) {
            Some(TabRef::Sftp(id))
        } else {
            None
        }
    }

    /// Whether a strip ref is pinned. Pinned tabs lead the strip.
    pub(crate) fn ref_pinned(&self, r: &TabRef) -> bool {
        match r {
            TabRef::Terminal(id) => self.tabs.iter().any(|t| t._id == *id && t.pinned),
            TabRef::Sftp(id) => self.sftp_tabs.iter().any(|t| t.id == *id && t.pinned),
            TabRef::Panel(_) => false,
        }
    }

    /// The strip order of `target` (`None` is the resident one), from
    /// whichever window's values are swapped in.
    pub(crate) fn order_of_window(&self, target: Option<window::Id>) -> &[TabRef] {
        if target == self.window_ctx.as_ref().map(|c| c.id) {
            return &self.tab_order;
        }
        match target {
            Some(id) => self.extra_windows.get(&id).map(|w| w.order.as_slice()).unwrap_or(&[]),
            None => self.window_ctx.as_ref().map(|c| c.order.as_slice()).unwrap_or(&self.tab_order),
        }
    }

    /// The cell a chip of the current window's strip reports its drawn
    /// rectangle into.
    pub(crate) fn strip_chip_cell(&self, id: Uuid) -> crate::widgets::BoundsCell {
        self.strip_chip_bounds
            .borrow_mut()
            .entry((self.cur_window(), id))
            .or_insert_with(crate::widgets::new_bounds_cell)
            .clone()
    }

    /// Where the chip of tab `id` was last drawn in `window`'s strip
    /// (`None` is the resident one), once it has been.
    pub(crate) fn strip_chip_rect(&self, window: Option<window::Id>, id: Uuid) -> Option<iced::Rectangle> {
        self.strip_chip_bounds
            .borrow()
            .get(&(window, id))
            .map(|cell| cell.get())
            .filter(|rect| rect.width > 0.0)
    }

    /// The slot of `target`'s strip (pinned tabs first, as drawn) the
    /// carried tab `tab` would take with the cursor at `cursor` in that
    /// window: after every chip of its own group (pinned or not) whose
    /// middle lies before the cursor along the strip.
    pub(crate) fn carry_slot(&self, target: Option<window::Id>, tab: Uuid, cursor: Point) -> usize {
        let pinned = self.ref_of_tab(tab).is_some_and(|r| self.ref_pinned(&r));
        let order = self.order_of_window(target);
        let pinned_count = order
            .iter()
            .filter(|r| r.strip_id() != tab && self.ref_pinned(r))
            .count();
        // The side strips run down, except for the pins when they live
        // in the top bar.
        let across = !crate::views::tab_bar::tab_bar_pos().is_side()
            || (pinned && self.vaults.cur().prefs.pinned_tabs_top_bar);
        // Across a mirrored (RTL) strip "before" is to the right.
        let rtl = crate::i18n::is_rtl_layout();
        let before = order
            .iter()
            .filter(|r| r.strip_id() != tab && self.ref_pinned(r) == pinned)
            .filter(|r| {
                self.strip_chip_rect(target, r.strip_id()).is_some_and(|rect| {
                    if across && rtl {
                        rect.center_x() > cursor.x
                    } else if across {
                        rect.center_x() < cursor.x
                    } else {
                        rect.center_y() < cursor.y
                    }
                })
            })
            .count();
        if pinned { before } else { pinned_count + before }
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
        self.give_ref_to_window_at(target, r, activate, None);
    }

    /// [`Self::give_ref_to_window`], landing at a slot of the target's
    /// strip as drawn (pinned tabs first) instead of at its end. The
    /// slot is kept inside the tab's own group.
    pub(crate) fn give_ref_to_window_at(
        &mut self,
        target: Option<window::Id>,
        r: TabRef,
        activate: bool,
        slot: Option<usize>,
    ) {
        let pinned_flags: Vec<(TabRef, bool)> = self
            .order_of_window(target)
            .iter()
            .map(|o| (*o, self.ref_pinned(o)))
            .collect();
        let r_pinned = self.ref_pinned(&r);
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
            let at = slot.map(|slot| storage_index(&pinned_flags, r_pinned, slot));
            match at {
                Some(at) => order.insert(at.min(order.len()), r),
                None => order.push(r),
            }
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
        // Chip rectangles of windows and tabs that are gone.
        let open: Vec<Option<window::Id>> = self.all_windows().into_iter().map(|(w, _)| w).collect();
        let tabs = &self.tabs;
        let sftp_tabs = &self.sftp_tabs;
        self.strip_chip_bounds.borrow_mut().retain(|(window, id), _| {
            open.contains(window)
                && (tabs.iter().any(|t| t._id == *id)
                    || sftp_tabs.iter().any(|t| t.id == *id)
                    || crate::state::PanelKind::ALL.iter().any(|k| k.tab_id() == *id))
        });
        for id in std::mem::take(&mut self.windows_revealing) {
            if self.all_windows().iter().any(|(_, w)| *w == id) {
                tasks.push(window::set_mode(id, window::Mode::Windowed));
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

/// Where in a strip's storage order a ref goes so that it is drawn at
/// `slot` (the drawn order puts the pinned refs first, each group in
/// storage order). `flags` is the storage order with each ref's pinned
/// flag; the slot is clamped into the ref's own group.
fn storage_index(flags: &[(TabRef, bool)], pinned: bool, slot: usize) -> usize {
    let pinned_count = flags.iter().filter(|(_, p)| *p).count();
    let within = if pinned {
        slot.min(pinned_count)
    } else {
        slot.saturating_sub(pinned_count)
    };
    // Before the `within`-th member of the group, or after its last.
    let mut seen = 0;
    let mut last = None;
    for (i, (_, p)) in flags.iter().enumerate() {
        if *p != pinned {
            continue;
        }
        if seen == within {
            return i;
        }
        seen += 1;
        last = Some(i);
    }
    match last {
        Some(i) => i + 1,
        // An empty group: pinned refs go first, the others last.
        None if pinned => 0,
        None => flags.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(pins: &[bool]) -> Vec<(TabRef, bool)> {
        pins.iter().map(|p| (TabRef::Terminal(Uuid::new_v4()), *p)).collect()
    }

    #[test]
    fn a_slot_lands_inside_its_own_group() {
        // Drawn: P0 P1 | U0 U1, stored interleaved: U0 P0 U1 P1.
        let order = flags(&[false, true, false, true]);
        // An unpinned tab at drawn slot 2 goes before U0, at 3 between
        // U0 and U1, at 4 (or past the end) right after U1: the pin
        // stored behind it is drawn first all the same.
        assert_eq!(storage_index(&order, false, 2), 0);
        assert_eq!(storage_index(&order, false, 3), 2);
        assert_eq!(storage_index(&order, false, 4), 3);
        assert_eq!(storage_index(&order, false, 9), 3);
        // A slot among the pins clamps to the first unpinned place.
        assert_eq!(storage_index(&order, false, 0), 0);
        // A pinned tab at 0 goes before P0, at 1 before P1, past the
        // pins after P1 and never among the unpinned.
        assert_eq!(storage_index(&order, true, 0), 1);
        assert_eq!(storage_index(&order, true, 1), 3);
        assert_eq!(storage_index(&order, true, 3), 4);
    }

    /// A view is built for one window while the fields hold another's
    /// (the resident's), so the helpers that read the SFTP owner out of
    /// the fields name the wrong surface in a second window: a view goes
    /// through `cur_sftp_owner` / `cur_hybrid_owner` / `cur_active_sftp`.
    #[test]
    fn views_read_the_sftp_owner_of_the_window_they_draw() {
        // Split so this test's own source does not match itself.
        let needles = [
            ["current_sftp_", "owner("].concat(),
            ["active_sftp_", "id("].concat(),
            [".hybrid_sftp_", "owner"].concat(),
            [".routing_", "sftp"].concat(),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("views");
        let mut stack = vec![root.clone()];
        let mut found: Vec<String> = Vec::new();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read views dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("read source");
                for (idx, line) in src.lines().enumerate() {
                    if needles.iter().any(|n| line.contains(n.as_str())) {
                        let rel = path.strip_prefix(&root).expect("under views");
                        found.push(format!("views/{}:{}", rel.display(), idx + 1));
                    }
                }
            }
        }
        assert!(found.is_empty(), "a view reads the SFTP owner from the fields at {found:?}; use cur_sftp_owner()");
    }

    #[test]
    fn an_empty_group_puts_pins_first_and_the_rest_last() {
        let order = flags(&[false, false]);
        assert_eq!(storage_index(&order, true, 5), 0);
        let order = flags(&[true]);
        assert_eq!(storage_index(&order, false, 0), 1);
        assert_eq!(storage_index(&[], false, 0), 0);
    }
}
