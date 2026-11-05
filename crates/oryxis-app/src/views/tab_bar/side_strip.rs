//! Side-docked (left / right) vertical tab strip (issue #87).
//!
//! When `tab_bar_position` is `left` or `right` the tabs stack as a
//! vertical list on that window edge. Every chip is rendered by the same
//! `strip_tab_element` the horizontal strips use, at one uniform row
//! width; compact pinned chips pack several per row (Edge-style pinned
//! grid). Left / right are physical edges by user choice, so RTL never
//! flips the strip; the chips' inner rows still mirror through
//! `dir_row` like everywhere else.
//!
//! Three side-only options compose on top (Settings -> Interface):
//! `pinned_tabs_top_bar` docks the pinned tabs with the window chrome
//! (the slim top bar, or a fixed group under this strip's header when
//! that bar is hidden); `side_hide_top_bar` removes the top bar and
//! moves the whole titlebar contract in here (header row with burger +
//! Home + compact window buttons, empty areas drag the window,
//! double-click maximizes); `side_full_height` is layout-only
//! (`main_layout` keeps the status bar off this strip's column).

use super::*;

/// Total width of the side-docked strip, gutters included.
pub(crate) const SIDE_STRIP_WIDTH: f32 = 216.0;
/// Uniform row width of every tab chip inside the strip (the strip
/// minus its 8px side gutters).
pub(crate) const SIDE_TAB_WIDTH: f32 = SIDE_STRIP_WIDTH - 16.0;
/// Rendered height of one strip row: a chip renders at `TAB_ROW_HEIGHT`
/// (content box + the button's default 5px top/bottom paddings).
const SIDE_ROW_HEIGHT: f32 = TAB_ROW_HEIGHT;
/// Compact window-chrome cell used by the strip header when the top
/// bar is hidden (the standard 46px cells would eat half the strip).
const HEADER_CHROME_W: f32 = 28.0;
const HEADER_CHROME_H: f32 = 32.0;

impl Oryxis {
    /// The vertical tab strip for the left / right docked layout: an
    /// optional titlebar header (hidden-top-bar mode), an optional
    /// fixed pinned group, then the scrolling tab list with `+`; `⋯`
    /// joins a docked footer once the list overflows the viewport.
    pub(crate) fn view_side_tab_strip(&self) -> Element<'_, Message> {
        // Collapsed icon rail (sidebar minimizer): the strip shrinks to
        // a narrow column of icon-only tab buttons with tooltips, a `+`,
        // and the expand toggle at the foot, mirroring the vault nav
        // rail's collapsed form. Returned as its own subtree, so the
        // full-strip assembly below is untouched. The whole rail is a
        // single constant-shape slot in `main_layout` (its width changes,
        // the slot does not), which keeps the scroll-state keying safe.
        if self.vaults.cur().prefs.sidebar_collapsed {
            return self.view_side_tab_rail();
        }
        let hide_top_bar = self.top_bar_hidden();
        let pins_top = self.vaults.cur().prefs.pinned_tabs_top_bar;
        let compact_pins = self.vaults.cur().prefs.pinned_tab_style == "compact";
        let solid_fill =
            self.vaults.cur().prefs.tab_fill_style == "solid" || self.vaults.cur().prefs.performance_mode;
        let ctx = StripCtx {
            privacy_terms: self.privacy_terms(),
            close_on_right: self.vaults.cur().prefs.tab_close_button_side == "right",
            close_armed: self.cur_nav().hover.tab_close_armed,
            compact_pins,
            solid_fill,
            uniform_w: Some(SIDE_TAB_WIDTH),
            session_widths: Vec::new(),
            number_px: self.tab_number_px(),
        };
        let chips_per_row = ((SIDE_TAB_WIDTH + TAB_SPACING)
            / (CHIP_W + TAB_SPACING))
            .floor()
            .max(1.0) as usize;

        // Fixed head (never scrolls). Hidden-top-bar mode: the titlebar
        // contract lives here: burger + Home + drag gap + side-panel
        // toggle + compact window chrome. `pinned_tabs_top_bar` then
        // docks the pinned tabs as an always-visible group right under
        // it (Zen-style essentials), since there is no top bar to host
        // them.
        let mut head: Vec<Element<'_, Message>> = Vec::new();
        if hide_top_bar {
            // macOS (left dock only, `top_bar_hidden`): the strip is the
            // top-left of the window, which is where the traffic lights
            // float. The header row is already full at 216 px, so they
            // get a title-bar band of their own above it rather than a
            // corner of it: a drag area as tall as AppKit's title bar,
            // which is what centres the buttons in it.
            if self.traffic_light_inset() > 0.0 {
                head.push(crate::views::chrome::traffic_light_gutter(
                    SIDE_STRIP_WIDTH,
                    crate::views::chrome::MACOS_TITLE_BAR_HEIGHT,
                ));
            }
            let mut header: Vec<Element<'_, Message>> = vec![
                burger_menu_btn(self.cur_burger_open()),
                self.home_area_tab(solid_fill, true),
                Space::new().width(Length::Fill).boxed(),
            ];
            if self.cur_active_tab().is_some() {
                for toggle_side in self.sidebar_toggle_sides() {
                    header.push(sidebar_btn(toggle_side, SIDEBAR_TOGGLE_WIDTH, HEADER_CHROME_H));
                }
            }
            header.push(self.window_chrome_row(HEADER_CHROME_W, HEADER_CHROME_H).boxed());
            head.push(
                crate::widgets::dir_row(header)
                    .align_y(iced::Alignment::Center)
                    .boxed(),
            );
        }
        let pins_docked_here = hide_top_bar && pins_top;
        let mut pins_row_count = 0usize;
        if pins_docked_here {
            let rows = self.side_pins_rows(&ctx, chips_per_row, compact_pins);
            pins_row_count = rows.len();
            head.extend(rows);
        }

        // Scrolling list. With `pinned_tabs_top_bar` the pinned entries
        // live with the chrome (top bar or the fixed head above), so
        // they are skipped here; otherwise consecutive compact pinned
        // chips pack into rows at the top of the list (`strip_order` is
        // pinned-first) and everything else stacks one chip per row.
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        let mut chip_row: Vec<Element<'_, Message>> = Vec::new();
        let mut row_count = 0usize;
        // Slots count the full strip order (see `strip_tab_element`):
        // with the pins docked in the top bar this list skips them, and
        // a local counter would renumber the rest from 1.
        for (slot, entry) in self.strip_order().into_iter().enumerate() {
            let pinned = self.strip_entry_pinned(entry);
            if pins_top && pinned {
                continue;
            }
            let el = self.strip_tab_element(&ctx, entry, slot);
            if compact_pins && pinned {
                chip_row.push(el);
                if chip_row.len() == chips_per_row {
                    items.push(
                        row(std::mem::take(&mut chip_row))
                            .spacing(TAB_SPACING)
                            .boxed(),
                    );
                    row_count += 1;
                }
            } else {
                if !chip_row.is_empty() {
                    items.push(
                        row(std::mem::take(&mut chip_row))
                            .spacing(TAB_SPACING)
                            .boxed(),
                    );
                    row_count += 1;
                }
                items.push(el);
                row_count += 1;
            }
        }
        if !chip_row.is_empty() {
            items.push(row(chip_row).spacing(TAB_SPACING).boxed());
            row_count += 1;
        }

        // Overflow: the rows (plus the trailing `+`) don't fit the
        // strip's viewport, so the `+` docks into a fixed footer with
        // the `⋯` jump button and the list alone scrolls. Mirrors the
        // horizontal strip's docked-plus / scroll-mode pair; vertical
        // rows never compress, so one trigger covers both.
        let strip_top = if hide_top_bar { 0.0 } else { BAR_HEIGHT + 1.0 };
        // The pinned dock is capped at ~40% of the window: an always-
        // visible head that outgrew the viewport would push the list
        // (and every unpinned tab) clean off screen with no way to
        // reach them. Past the cap the dock scrolls on its own.
        let pins_h_raw = pins_row_count as f32 * (SIDE_ROW_HEIGHT + TAB_SPACING);
        let pins_h_max =
            (self.cur_window_size().height * 0.4).max(3.0 * (SIDE_ROW_HEIGHT + TAB_SPACING));
        let pins_overflow = pins_h_raw > pins_h_max;
        let head_h = if hide_top_bar { BAR_HEIGHT } else { 0.0 }
            + pins_h_raw.min(pins_h_max);
        let viewport_h =
            (self.cur_window_size().height - strip_top - head_h - 40.0).max(120.0);
        let content_h = (row_count as f32 + 1.0) * (SIDE_ROW_HEIGHT + TAB_SPACING);
        let overflow = content_h > viewport_h;

        // Same `+` affordance as the horizontal strip: bounds reported
        // for the split-menu anchor, drag-to-end drop target.
        let plus_btn: Element<'_, Message> = MouseArea::new(crate::widgets::bounds_reporter(
            new_tab_btn(!overflow),
            self.plus_btn_bounds.clone(),
        ))
        .on_enter(Message::Tabs(TabsMessage::TabDragToEnd))
        .boxed();
        // Visible collapse toggle on the full strip, so minimizing the
        // sidebar is one obvious click (the right-click menu entry is
        // easy to miss). It lives in the strip FOOTER (pinned to the
        // bottom edge by the Fill scrollable above it), not in the
        // scrolling list: floating it under the `+` mid-strip read as an
        // orphaned control. Tucked into the bottom-TRAILING corner (the
        // content-facing edge), the standard spot for a panel collapse.
        // The `+` stays with the tabs.
        let footer: Option<Element<'_, Message>> = if overflow {
            // Overflow docks the `+` and jump button here too; a leading
            // Fill pushes the collapse toggle to the trailing corner, so
            // the bottom row reads [ + · ⋯ ············ collapse ].
            Some(
                row(vec![
                    plus_btn,
                    Space::new().width(2).boxed(),
                    tab_jump_btn(),
                    Space::new().width(Length::Fill).boxed(),
                    side_strip_collapse_item(false, false),
                ])
                    .align_y(iced::Alignment::Center)
                    .boxed(),
            )
        } else {
            items.push(plus_btn);
            // Lone toggle, trailing-aligned so it sits in the bottom-right
            // corner of the strip (its content-facing edge).
            Some(side_strip_collapse_item(false, true))
        };

        // Vertical scrollable, scrollbar zeroed out like the horizontal
        // strip (the wheel still scrolls it natively).
        let strip_scroll = scrollable(
            iced::widget::Column::with_children(items).spacing(TAB_SPACING),
        )
        .id(iced::widget::Id::new("tab-scroll"))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(0.0).scroller_width(0.0),
        ))
        .width(Length::Fill)
        .height(Length::Fill);

        // Constant-shape column: the header, pinned dock, list and
        // footer live in FIXED child slots (zero-sized Spaces when
        // absent). iced pairs widget state by child position and every
        // stateless widget shares one tag, so a slot that appears or
        // disappears would silently shift the scrollable one position
        // over and hand its state (the scroll offset) to a sibling.
        // The placeholder must be a SHRINK Space: this fork's
        // Column::push void-filters any zero-FIXED child, which would
        // silently drop the slot (see the skeleton note in
        // `main_layout.rs`).
        let empty = || -> Element<'_, Message> { Space::new().boxed() };
        let mut head_slots = head.into_iter();
        let header_slot = head_slots.next().unwrap_or_else(empty);
        let pins_slot: Element<'_, Message> = {
            let rest: Vec<Element<'_, Message>> = head_slots.collect();
            if rest.is_empty() {
                empty()
            } else {
                let dock = iced::widget::Column::with_children(rest).spacing(TAB_SPACING);
                if pins_overflow {
                    scrollable(dock)
                        .id(iced::widget::Id::new("side-pins-scroll"))
                        .direction(scrollable::Direction::Vertical(
                            scrollable::Scrollbar::new().width(0.0).scroller_width(0.0),
                        ))
                        .width(Length::Fill)
                        .height(Length::Fixed(pins_h_max))
                        .boxed()
                } else {
                    dock.boxed()
                }
            }
        };
        // A hair of breathing room above/below the pinned footer so the
        // collapse toggle doesn't sit flush against the tabs or the
        // strip's bottom edge.
        let footer_slot = footer
            .map(|f| {
                container(f)
                    .padding(Padding { top: 6.0, right: 0.0, bottom: 4.0, left: 0.0 })
                    .boxed()
            })
            .unwrap_or_else(empty);
        let inner = iced::widget::Column::with_children(vec![
            header_slot,
            pins_slot,
            strip_scroll.boxed(),
            footer_slot,
        ])
        .spacing(TAB_SPACING);

        // Strip surface: a UNIFORM accent tint, not a gradient. The old
        // top->bottom fade (accent at the top fading to base by 90% down)
        // read as a washed-out, dimmer lower half — the strip looked
        // like it was fading into the status bar. A single flat tint
        // keeps the accent character the `tab_accent_wash` setting asks
        // for while rendering the whole column evenly, top to bottom.
        let bar_base = OryxisColors::t().bg_sidebar;
        let bar_bg = if self.vaults.cur().prefs.tab_accent_wash {
            Background::Color(crate::theme::mix(bar_base, self.top_accent_tint(), 0.16))
        } else {
            Background::Color(bar_base)
        };
        let mut bar: Element<'_, Message> = container(inner)
            // 8px top/bottom to match the Settings nav rail's inset, so
            // both sidebars breathe the same at top and bottom edges.
            .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
            .width(Length::Fixed(SIDE_STRIP_WIDTH))
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(bar_bg),
                ..Default::default()
            })
            .boxed();
        // With no top bar this strip IS the titlebar: its empty areas
        // (header gap, space below the tabs) drag the window and
        // double-click maximizes, exactly like the horizontal strips.
        // Tab buttons consume their own presses, so clicks on chips
        // never start a window move. The MouseArea wrapper is ALWAYS
        // present (handlers only bound in hidden-bar mode) so the
        // hide toggle never changes this subtree's widget type.
        // The strip menu is bound whatever the top bar is doing: it
        // belongs to the tabs, not to the titlebar contract above
        // (issue #186).
        let mut area = MouseArea::new(bar)
            .on_right_press(Message::Tabs(TabsMessage::ShowTabBarMenu));
        if hide_top_bar {
            area = area
                .on_press(Message::Tabs(TabsMessage::WindowDrag))
                .on_double_click(Message::Tabs(TabsMessage::WindowMaximizeToggle));
        }
        bar = area.boxed();

        // Floating drag ghost, tracking the cursor's y (the horizontal
        // bars track x). Non-interactive so the tab MouseAreas below
        // keep their hover.
        // With the pins docked in the (visible) top bar, that bar draws
        // the pinned ghosts instead.
        // Leaving the strip hands the ghost to the window-level layer,
        // which is the only one that can follow the cursor into the
        // content area (issue #112).
        let ghost_elsewhere = (pins_top && !hide_top_bar && self.dragged_tab_pinned())
            || !self.cursor_in_tab_strip();
        let ghost = (!ghost_elsewhere)
            .then(|| {
                self.strip_drag_ghost_el(Some(SIDE_TAB_WIDTH), compact_pins, &ctx.privacy_terms)
            })
            .flatten()
            .map(|(ghost, _ghost_w)| {
                let gy = (self.strip_cursor().y - strip_top - 6.0 - SIDE_ROW_HEIGHT / 2.0)
                    .max(0.0);
                iced::widget::Column::<iced::Element<'_, _>>::new()
                    .push(Space::new().height(gy).boxed())
                    .push(
                        iced::widget::Row::<iced::Element<'_, _>>::new()
                            .push(Space::new().width(8.0).boxed())
                            .push(ghost).boxed(),
                    )
                    .boxed()
            });
        with_ghost_layer(bar, ghost, Length::Fixed(SIDE_STRIP_WIDTH), Length::Fill)
    }

    /// The collapsed icon-only form of the side strip: a scrolling
    /// column of `side_rail_item` buttons (one per `strip_order` entry),
    /// the `+` new-tab button, and a pinned expand toggle at the foot.
    /// No drag-reorder, no pinned packing, no header chrome: the rail is
    /// the minimal glance surface. Drag-reorder stays a full-strip
    /// affordance (the icons are too small to aim a reorder at), so the
    /// ghost layer is omitted.
    fn view_side_tab_rail(&self) -> Element<'_, Message> {
        let bar_base = OryxisColors::t().bg_sidebar;

        let mut items: Vec<Element<'_, Message>> = self
            .strip_order()
            .into_iter()
            .map(|entry| self.side_rail_item(entry))
            .collect();
        // Same `+` affordance as the full strip, centered in the rail.
        items.push(
            container(new_tab_btn(true))
                .width(Length::Fill)
                .center_x(Length::Fill)
                .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 })
                .boxed(),
        );

        let list = scrollable(
            iced::widget::Column::with_children(items)
                .spacing(4)
                .padding(Padding { top: 8.0, right: 0.0, bottom: 0.0, left: 0.0 }),
        )
        .id(iced::widget::Id::new("tab-scroll"))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(0.0).scroller_width(0.0),
        ))
        .width(Length::Fill)
        .height(Length::Fill);

        let footer = container(side_strip_collapse_item(true, false))
            .padding(Padding { top: 8.0, right: 0.0, bottom: 12.0, left: 0.0 });

        let inner = iced::widget::Column::with_children(vec![list.boxed(), footer.boxed()])
            .width(Length::Fill)
            .height(Length::Fill);

        let bar: Element<'_, Message> = container(inner)
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
            .width(Length::Fixed(crate::app::SIDEBAR_WIDTH_COLLAPSED))
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(bar_base)),
                ..Default::default()
            })
            .boxed();
        // Keep the right-press strip menu reachable on the rail too.
        MouseArea::new(bar)
            .on_right_press(Message::Tabs(TabsMessage::ShowTabBarMenu))
            .boxed()
    }

    /// The pinned `strip_order` entries as fixed strip rows: compact
    /// chips packed `chips_per_row` per row, full-style pins one per
    /// row. Used by the hidden-top-bar head when `pinned_tabs_top_bar`
    /// docks the pins on this strip.
    fn side_pins_rows(
        &self,
        ctx: &StripCtx,
        chips_per_row: usize,
        compact_pins: bool,
    ) -> Vec<Element<'_, Message>> {
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        let mut chip_row: Vec<Element<'_, Message>> = Vec::new();
        for (slot, entry) in self.strip_order().into_iter().enumerate() {
            if !self.strip_entry_pinned(entry) {
                continue;
            }
            let el = self.strip_tab_element(ctx, entry, slot);
            if compact_pins {
                chip_row.push(el);
                if chip_row.len() == chips_per_row {
                    rows.push(
                        row(std::mem::take(&mut chip_row))
                            .spacing(TAB_SPACING)
                            .boxed(),
                    );
                }
            } else {
                rows.push(el);
            }
        }
        if !chip_row.is_empty() {
            rows.push(row(chip_row).spacing(TAB_SPACING).boxed());
        }
        rows
    }
}

/// The side strip's collapse / expand toggle, firing
/// `ToggleSidebarCollapsed`. `collapsed` picks the direction: on the
/// collapsed rail it shows the "open" glyph and an "expand" tooltip; on
/// the full strip the "close" glyph and a "collapse" tooltip. RTL-aware
/// (the strip grows toward the physical content edge), icon-only in both
/// so it fits the 56px rail and the full strip's footer alike.
///
/// `lead_align` tucks the button against the strip's LEADING edge (the
/// bottom-left corner on an LTR left dock) with the strip's own 8px
/// gutter, which is where the full strip wants it; the collapsed rail
/// passes `false` to keep it centered in its narrow column.
fn side_strip_collapse_item<'a>(collapsed: bool, trail_align: bool) -> Element<'a, Message> {
    let rtl = crate::i18n::is_rtl_layout();
    let icon = match (rtl, collapsed) {
        (false, true) => iced_fonts::lucide::panel_left_open(),
        (false, false) => iced_fonts::lucide::panel_left_close(),
        (true, true) => iced_fonts::lucide::panel_right_open(),
        (true, false) => iced_fonts::lucide::panel_right_close(),
    };
    let tip = crate::i18n::t(if collapsed { "expand_sidebar" } else { "collapse_sidebar" });
    let muted = OryxisColors::t().text_secondary;
    let btn = button(container(icon.size(16).color(muted)).center(Length::Fixed(40.0)))
        .on_press(Message::Tabs(TabsMessage::ToggleSidebarCollapsed))
        .padding(0)
        .width(Length::Fixed(40.0))
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                BtnStatus::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.14),
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }
        });
    // Trailing-aligned: sit against the strip's TRAILING gutter (its
    // content-facing edge, the bottom-right corner on an LTR left dock),
    // the standard spot for a panel collapse toggle. Under RTL the
    // trailing edge flips to the left, keeping it content-facing.
    let trailing = if crate::i18n::is_rtl_layout() {
        iced::alignment::Horizontal::Left
    } else {
        iced::alignment::Horizontal::Right
    };
    let holder = if trail_align {
        container(btn)
            .width(Length::Fill)
            .align_x(trailing)
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 })
    } else {
        container(btn)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 })
    };
    iced::widget::tooltip(
        holder,
        container(text(tip).size(11).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Right,
    )
    .boxed()
}
